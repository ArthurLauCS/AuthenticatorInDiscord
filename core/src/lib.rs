//! 验证器引擎（和界面无关，单独测试）：TOTP 计算（RFC 6238）、二维码图片识别、
//! otpauth://totp 与 Google 验证器导出链接（otpauth-migration://）解析、条目列表的存储格式和导入去重。

use hmac::{Hmac, KeyInit, Mac};

mod qr;
mod vault;

pub use qr::{decode_gray, decode_image};
pub use vault::{Entry, Vault};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Algo {
    Sha1,
    Sha256,
    Sha512,
}

/// 一个 TOTP 验证器；密钥、算法、位数、周期都相同即视为同一个（导入去重按这个）
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Otp {
    pub secret: Vec<u8>,
    pub algo: Algo,
    pub digits: u32,
    pub period: u64,
}

/// 从链接里解析出的账号
#[derive(Clone, PartialEq, Debug)]
pub struct Account {
    pub issuer: String,
    pub name: String,
    pub otp: Otp,
}

/// 一条链接的解析结果；unsupported 是不支持而跳过的账号数（HOTP、MD5 等）
#[derive(Default, Debug, PartialEq)]
pub struct Parsed {
    pub accounts: Vec<Account>,
    pub unsupported: usize,
}

impl Otp {
    /// unix 秒时刻的验证码
    pub fn code(&self, unix: u64) -> String {
        let msg = (unix / self.period).to_be_bytes();
        macro_rules! mac {
            ($d:ty) => {{
                let mut m = Hmac::<$d>::new_from_slice(&self.secret).expect("HMAC 接受任意长度的密钥");
                m.update(&msg);
                m.finalize().into_bytes().to_vec()
            }};
        }
        let h = match self.algo {
            Algo::Sha1 => mac!(sha1::Sha1),
            Algo::Sha256 => mac!(sha2::Sha256),
            Algo::Sha512 => mac!(sha2::Sha512),
        };
        let o = (h[h.len() - 1] & 0xF) as usize;
        let n = u32::from_be_bytes([h[o] & 0x7F, h[o + 1], h[o + 2], h[o + 3]]) % 10u32.pow(self.digits);
        format!("{n:0w$}", w = self.digits as usize)
    }

    /// 当前码还剩几秒有效（1..=period）
    pub fn remaining(&self, unix: u64) -> u64 {
        self.period - unix % self.period
    }
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha1 => "SHA1",
            Algo::Sha256 => "SHA256",
            Algo::Sha512 => "SHA512",
        }
    }

    pub fn from_name(s: &str) -> Option<Algo> {
        match s.to_ascii_uppercase().as_str() {
            "SHA1" => Some(Algo::Sha1),
            "SHA256" => Some(Algo::Sha256),
            "SHA512" => Some(Algo::Sha512),
            _ => None,
        }
    }
}

/// RFC 4648 Base32，忽略大小写、空格、横线和补位等号
pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut buf, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes().filter(|c| !matches!(c, b' ' | b'-' | b'=')) {
        let v = match c.to_ascii_uppercase() {
            u @ b'A'..=b'Z' => u - b'A',
            d @ b'2'..=b'7' => d - b'2' + 26,
            _ => return None,
        };
        buf = (buf << 5 | v as u32) & 0xFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

pub fn base32_encode(b: &[u8]) -> String {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut out, mut buf, mut bits) = (String::new(), 0u32, 0);
    for &x in b {
        buf = (buf << 8 | x as u32) & 0xFFF;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[(buf >> bits & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(A[(buf << (5 - bits) & 31) as usize] as char);
    }
    out
}

/// Base64，标准和 URL 两种字母表都认，补位可省
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut buf, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        buf = (buf << 6 | v as u32) & 0xFFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// URL 百分号解码；“+”原样保留（导出链接里的 Base64 可能没转义加号）
fn pct(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let hex = (b[i] == b'%').then(|| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        match hex {
            Some(x) => {
                out.push(x);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(q: &str) -> impl Iterator<Item = (String, String)> + '_ {
    q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_ascii_lowercase(), pct(v)))
}

/// 解析一条链接（二维码内容）；不是 otpauth / otpauth-migration 链接或内容损坏时返回 None
pub fn parse(uri: &str) -> Option<Parsed> {
    let uri = uri.trim();
    let (scheme, rest) = uri.split_once("://")?;
    match scheme.to_ascii_lowercase().as_str() {
        "otpauth-migration" => parse_migration(rest),
        "otpauth" => parse_otpauth(rest),
        _ => None,
    }
}

/// otpauth://totp/发行方:账号?secret=…&issuer=…&algorithm=…&digits=…&period=…（Google Key URI 格式）
fn parse_otpauth(rest: &str) -> Option<Parsed> {
    let (kind, rest) = rest.split_once('/')?;
    if !kind.eq_ignore_ascii_case("totp") {
        return kind.eq_ignore_ascii_case("hotp").then(|| Parsed { accounts: Vec::new(), unsupported: 1 });
    }
    let (label, q) = rest.split_once('?').unwrap_or((rest, ""));
    let label = pct(label);
    let (mut issuer, name) = match label.split_once(':') {
        Some((i, n)) => (i.trim().to_string(), n.trim().to_string()),
        None => (String::new(), label.trim().to_string()),
    };
    let (mut secret, mut algo, mut digits, mut period) = (None, Algo::Sha1, 6, 30);
    for (k, v) in query(q) {
        match k.as_str() {
            "secret" => secret = base32_decode(&v),
            "issuer" if !v.trim().is_empty() => issuer = v.trim().to_string(),
            "algorithm" => match Algo::from_name(&v) {
                Some(a) => algo = a,
                None => return Some(Parsed { accounts: Vec::new(), unsupported: 1 }),
            },
            "digits" => digits = v.parse().ok()?,
            "period" => period = v.parse().ok()?,
            _ => {}
        }
    }
    let secret = secret.filter(|s| !s.is_empty())?;
    // 周期为 0 会除零；位数只有 6、8 两种（Google 验证器同样只支持这两种）
    if period == 0 || !matches!(digits, 6 | 8) {
        return Some(Parsed { accounts: Vec::new(), unsupported: 1 });
    }
    Some(Parsed { accounts: vec![Account { issuer, name, otp: Otp { secret, algo, digits, period } }], unsupported: 0 })
}

/// otpauth-migration://offline?data=<Base64 的 protobuf>。一批导出可能分成好几张二维码，每张单独解析即可，
/// 不要求凑齐同一批；导出格式只有 TOTP 周期固定 30 秒
fn parse_migration(rest: &str) -> Option<Parsed> {
    let (_, q) = rest.split_once('?')?;
    let data = query(q).find(|(k, _)| k == "data")?.1;
    let raw = base64_decode(&data)?;
    let mut out = Parsed::default();
    for (field, v) in proto(&raw)? {
        let (1, Val::Bytes(p)) = (field, v) else { continue };
        let (mut secret, mut name, mut issuer, mut algo, mut digits, mut kind) = (Vec::new(), String::new(), String::new(), 0, 0, 0);
        for (f, v) in proto(p)? {
            match (f, v) {
                (1, Val::Bytes(b)) => secret = b.to_vec(),
                (2, Val::Bytes(b)) => name = String::from_utf8_lossy(b).trim().to_string(),
                (3, Val::Bytes(b)) => issuer = String::from_utf8_lossy(b).trim().to_string(),
                (4, Val::Int(n)) => algo = n,
                (5, Val::Int(n)) => digits = n,
                (6, Val::Int(n)) => kind = n,
                _ => {}
            }
        }
        // 枚举值见 proto/googleauth.proto；未指定时按 Google 验证器的默认值（SHA1、6 位）
        let algo = match algo {
            0 | 1 => Some(Algo::Sha1),
            2 => Some(Algo::Sha256),
            3 => Some(Algo::Sha512),
            _ => None,
        };
        let digits = match digits {
            0 | 1 => Some(6),
            2 => Some(8),
            _ => None,
        };
        match (kind, algo, digits) {
            (2, Some(algo), Some(digits)) if !secret.is_empty() => {
                // 账号名常带“发行方:”前缀，和发行方重复时去掉
                let name = name.strip_prefix(&format!("{issuer}:")).map(|n| n.trim().to_string()).unwrap_or(name);
                out.accounts.push(Account { issuer, name, otp: Otp { secret, algo, digits, period: 30 } });
            }
            _ => out.unsupported += 1,
        }
    }
    Some(out)
}

enum Val<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

/// 把一段 protobuf 拆成 (字段号, 值)；格式错误返回 None
fn proto(mut b: &[u8]) -> Option<Vec<(u64, Val<'_>)>> {
    fn varint(b: &mut &[u8]) -> Option<u64> {
        let mut v = 0u64;
        for i in 0..10 {
            let (&c, rest) = b.split_first()?;
            *b = rest;
            v |= ((c & 0x7F) as u64) << (7 * i);
            if c & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !b.is_empty() {
        let key = varint(&mut b)?;
        let v = match key & 7 {
            0 => Val::Int(varint(&mut b)?),
            2 => {
                let n = varint(&mut b)? as usize;
                if n > b.len() {
                    return None;
                }
                let (x, rest) = b.split_at(n);
                b = rest;
                Val::Bytes(x)
            }
            w @ (1 | 5) => {
                let n = if w == 1 { 8 } else { 4 };
                b = b.get(n..)?;
                continue;
            }
            _ => return None,
        };
        out.push((key >> 3, v));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238() {
        let otp = |s: &[u8], algo| Otp { secret: s.to_vec(), algo, digits: 8, period: 30 };
        let s1 = otp(b"12345678901234567890", Algo::Sha1);
        let s256 = otp(b"12345678901234567890123456789012", Algo::Sha256);
        let s512 = otp(b"1234567890123456789012345678901234567890123456789012345678901234", Algo::Sha512);
        for (t, a, b, c) in [(59, "94287082", "46119246", "90693936"), (1111111109, "07081804", "68084774", "25091201"), (20000000000, "65353130", "77737706", "47863826")] {
            assert_eq!((s1.code(t).as_str(), s256.code(t).as_str(), s512.code(t).as_str()), (a, b, c));
        }
        assert_eq!((s1.remaining(59), s1.remaining(60)), (1, 30));
    }

    #[test]
    fn base32() {
        assert_eq!(base32_decode("jbsw y3dp ehpk 3pxp").unwrap(), b"Hello!\xde\xad\xbe\xef");
        for n in 0..12 {
            let b: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&b)).unwrap(), b);
        }
        assert!(base32_decode("ABC1").is_none());
    }

    #[test]
    fn migration() {
        // Google 验证器测试用例里的导出链接：没有算法、位数字段（取默认值），账号名带发行方前缀
        let p = parse("otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC").unwrap();
        assert_eq!(p.unsupported, 0);
        let a = &p.accounts[0];
        assert_eq!((a.issuer.as_str(), a.name.as_str()), ("Example", "alice@google.com"));
        assert_eq!(a.otp, Otp { secret: b"Hello!\xde\xad\xbe\xef".to_vec(), algo: Algo::Sha1, digits: 6, period: 30 });

        // 自己拼一份：一个 SHA256 / 8 位 TOTP 加一个 HOTP，data 做了百分号转义
        let item = |kind: u8, algo: u8, digits: u8| {
            let mut v = vec![0x0A, 3, 1, 2, 3, 0x12, 1, b'n', 0x1A, 1, b'i', 0x20, algo, 0x28, digits, 0x30, kind];
            v.splice(0..0, [0x0A, v.len() as u8]);
            v
        };
        let mut raw = [item(2, 2, 2), item(1, 1, 1)].concat();
        raw.extend([0x10, 1, 0x18, 1]);
        let b64 = base64_encode(&raw).replace('+', "%2B").replace('/', "%2F").replace('=', "%3D");
        let p = parse(&format!("otpauth-migration://offline?data={b64}")).unwrap();
        assert_eq!(p.unsupported, 1);
        assert_eq!(p.accounts, vec![Account { issuer: "i".into(), name: "n".into(), otp: Otp { secret: vec![1, 2, 3], algo: Algo::Sha256, digits: 8, period: 30 } }]);

        assert!(parse("otpauth-migration://offline?data=CjEK%%%").is_none());
        assert!(parse("otpauth-migration://offline?data=CjEKCkhl").is_none());
    }

    #[test]
    fn otpauth() {
        let p = parse("otpauth://totp/ACME%20Co:john@example.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA256&digits=8&period=60").unwrap();
        let a = &p.accounts[0];
        assert_eq!((a.issuer.as_str(), a.name.as_str(), a.otp.algo, a.otp.digits, a.otp.period), ("ACME Co", "john@example.com", Algo::Sha256, 8, 60));
        let p = parse("OTPAUTH://totp/bob?secret=JBSWY3DPEHPK3PXP").unwrap();
        assert_eq!((p.accounts[0].issuer.as_str(), p.accounts[0].name.as_str(), p.accounts[0].otp.digits), ("", "bob", 6));
        assert_eq!(parse("otpauth://hotp/x?secret=JBSWY3DPEHPK3PXP&counter=1").unwrap().unsupported, 1);
        assert_eq!(parse("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP&period=0").unwrap().unsupported, 1);
        assert!(parse("otpauth://totp/x?issuer=a").is_none());
        assert!(parse("https://example.com").is_none());
    }

    fn base64_encode(b: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        b.chunks(3)
            .flat_map(|c| {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                (0..4).map(move |i| if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' })
            })
            .collect()
    }
}

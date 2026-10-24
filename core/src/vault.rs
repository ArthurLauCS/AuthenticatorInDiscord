//! 条目列表和它的明文格式；磁贴用 DPAPI 加密后存盘。
//! 首行 `next|下一个编号`，之后每行一条，字段用制表符分隔：编号、显示名、发行方、账号、Base32 密钥、算法、位数、周期。

use crate::{Account, Algo, Otp, base32_decode, base32_encode};

#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    /// 不随改名、排序变化，菜单、改名框靠它找条目
    pub id: u32,
    /// 显示名（磁贴的行名），可改；可以带 emoji
    pub label: String,
    pub issuer: String,
    pub name: String,
    pub otp: Otp,
}

#[derive(Default, Debug, PartialEq)]
pub struct Vault {
    pub entries: Vec<Entry>,
    /// 编号只增不复用：条目删掉后，还开着的菜单、改名框不会指到后来导入的条目
    next: u32,
}

/// 名字里的制表符、换行会破坏存储格式，换成空格
fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

impl Vault {
    pub fn parse(s: &str) -> Vault {
        let mut v = Vault::default();
        for l in s.lines() {
            if let Some(n) = l.strip_prefix("next|") {
                v.next = n.parse().unwrap_or(0);
                continue;
            }
            let f: Vec<&str> = l.split('\t').collect();
            let [id, label, issuer, name, secret, algo, digits, period] = f[..] else { continue };
            let (Ok(id), Some(secret), Some(algo), Ok(digits), Ok(period)) = (id.parse(), base32_decode(secret), Algo::from_name(algo), digits.parse(), period.parse()) else { continue };
            v.entries.push(Entry { id, label: label.into(), issuer: issuer.into(), name: name.into(), otp: Otp { secret, algo, digits, period } });
        }
        v.next = v.next.max(v.entries.iter().map(|e| e.id + 1).max().unwrap_or(1));
        v
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("next|{}\n", self.next);
        for e in &self.entries {
            let o = &e.otp;
            s += &format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", e.id, e.label, e.issuer, e.name, base32_encode(&o.secret), o.algo.name(), o.digits, o.period);
        }
        s
    }

    /// 导入账号，追加到末尾。已有相同验证器（同一批里重复的也算）的跳过，保留原来的名字。返回 (新增数, 重复数)
    pub fn import(&mut self, accounts: Vec<Account>) -> (usize, usize) {
        let before = self.entries.len();
        let total = accounts.len();
        for a in accounts {
            if self.entries.iter().any(|e| e.otp == a.otp) {
                continue;
            }
            let (issuer, name) = (clean(&a.issuer), clean(&a.name));
            let label = if issuer.is_empty() { name.clone() } else { issuer.clone() };
            self.next = self.next.max(1);
            self.entries.push(Entry { id: self.next, label, issuer, name, otp: a.otp });
            self.next += 1;
        }
        let added = self.entries.len() - before;
        (added, total - added)
    }

    /// 导入二维码里的文字（otpauth 链接或 Google 验证器导出链接）。一条都认不出时返回 None，否则返回 (新增数, 重复数, 不支持数)
    pub fn import_uris(&mut self, uris: &[String]) -> Option<(usize, usize, usize)> {
        let parsed: Vec<_> = uris.iter().filter_map(|u| crate::parse(u)).collect();
        if parsed.is_empty() {
            return None;
        }
        let unsupported = parsed.iter().map(|p| p.unsupported).sum();
        let (added, dup) = self.import(parsed.into_iter().flat_map(|p| p.accounts).collect());
        Some((added, dup, unsupported))
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 改显示名；清理后为空时不改，返回是否改了
    pub fn rename(&mut self, id: u32, label: &str) -> bool {
        let label = clean(label);
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) if !label.is_empty() => {
                e.label = label;
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let n = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != n
    }

    /// 往前（-1）或往后（+1）挪一位；到头了不动
    pub fn shift(&mut self, id: u32, delta: isize) -> bool {
        let Some(i) = self.entries.iter().position(|e| e.id == id) else { return false };
        let j = i as isize + delta;
        if j < 0 || j >= self.entries.len() as isize {
            return false;
        }
        self.entries.swap(i, j as usize);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(issuer: &str, name: &str, secret: u8) -> Account {
        Account { issuer: issuer.into(), name: name.into(), otp: Otp { secret: vec![secret; 10], algo: Algo::Sha1, digits: 6, period: 30 } }
    }

    #[test]
    fn import_dedup_roundtrip() {
        let mut v = Vault::default();
        assert_eq!(v.import(vec![acc("GitHub", "me", 1), acc("", "bob\tx", 2), acc("Dup", "same secret", 1)]), (2, 1));
        assert_eq!(v.entries.iter().map(|e| (e.id, e.label.as_str())).collect::<Vec<_>>(), [(1, "GitHub"), (2, "bob x")]);
        // 再次导入相同密钥：跳过，保留改过的名字
        assert!(v.rename(1, "🐻 熊熊子"));
        assert!(!v.rename(1, " \t "));
        assert_eq!(v.import(vec![acc("GitHub", "me", 1), acc("New", "n", 3)]), (1, 1));
        assert!(v.shift(3, -1) && !v.shift(1, -1));
        assert!(v.remove(2));
        // 删掉的编号不复用
        v.import(vec![acc("Later", "l", 4)]);
        assert_eq!(v.entries.iter().map(|e| e.id).collect::<Vec<_>>(), [1, 3, 4]);
        let back = Vault::parse(&v.to_text());
        assert_eq!(back, v);
        assert_eq!(back.get(1).unwrap().label, "🐻 熊熊子");
        assert_eq!(Vault::parse("next|9\nbad line\n1\tx\t\t\tNOT-B32!\tSHA1\t6\t30\n").next, 9);
    }

    #[test]
    fn import_uris_counts() {
        let mut v = Vault::default();
        let uris = ["otpauth://totp/A:me?secret=JBSWY3DPEHPK3PXP", "otpauth://hotp/B?secret=JBSWY3DPEHPK3PXP&counter=1", "https://example.invalid"];
        assert_eq!(v.import_uris(&uris.map(String::from)), Some((1, 0, 1)));
        assert_eq!(v.import_uris(&uris[..1].iter().map(|s| s.to_string()).collect::<Vec<_>>()), Some((0, 1, 0)));
        assert_eq!(v.import_uris(&["hello".into()]), None);
        assert_eq!(v.import_uris(&[]), None);
    }
}

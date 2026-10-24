//! 生成测试用二维码图片（假数据，测试导入、截 README 图用）。
//! cargo run -p authcore --example mkqr -- <输出.png> <项>...
//! 项是 `发行方|账号|Base32密钥` 时，所有这样的项合成一张 Google 验证器导出码；以 otpauth 开头的项各自单独成一张码。多张码横排在一张图里。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, items) = args.split_first().expect("用法：mkqr <输出.png> <发行方|账号|密钥 或 otpauth://…>...");
    let mut texts: Vec<String> = items.iter().filter(|s| s.starts_with("otpauth")).cloned().collect();
    let mut payload = Vec::new();
    for it in items.iter().filter(|s| !s.starts_with("otpauth")) {
        let [issuer, name, secret] = it.split('|').collect::<Vec<_>>()[..] else { panic!("格式应为 发行方|账号|密钥：{it}") };
        let secret = authcore::base32_decode(secret).expect("密钥不是 Base32");
        // OtpParameters：1 密钥、2 账号、3 发行方、4 算法 SHA1、5 位数 6、6 类型 TOTP
        let mut p = Vec::new();
        for (field, bytes) in [(1u8, secret.as_slice()), (2, name.as_bytes()), (3, issuer.as_bytes())] {
            p.extend([field << 3 | 2, bytes.len() as u8]);
            p.extend_from_slice(bytes);
        }
        p.extend([0x20, 1, 0x28, 1, 0x30, 2]);
        payload.extend([0x0A, p.len() as u8]);
        payload.extend(p);
    }
    if !payload.is_empty() {
        payload.extend([0x10, 1, 0x18, 1]);
        texts.push(format!("otpauth-migration://offline?data={}", b64(&payload).replace('+', "%2B").replace('/', "%2F").replace('=', "%3D")));
    }
    let codes: Vec<_> = texts.iter().map(|t| qrcode::QrCode::new(t).unwrap()).collect();
    let (cell, quiet) = (6, 4);
    let side = codes.iter().map(|c| c.width()).max().unwrap() + 2 * quiet;
    let mut img = image::GrayImage::from_pixel((side * cell * codes.len()) as u32, (side * cell) as u32, image::Luma([255]));
    for (k, c) in codes.iter().enumerate() {
        for (i, col) in c.to_colors().iter().enumerate() {
            if *col == qrcode::Color::Dark {
                let (x, y) = ((i % c.width() + quiet) * cell + k * side * cell, (i / c.width() + quiet) * cell);
                for d in 0..cell * cell {
                    img.put_pixel((x + d % cell) as u32, (y + d / cell) as u32, image::Luma([0]));
                }
            }
        }
    }
    img.save(out).unwrap();
    println!("{}", texts.join("\n"));
}

fn b64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    b.chunks(3)
        .flat_map(|c| {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            (0..4).map(move |i| if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' })
        })
        .collect()
}

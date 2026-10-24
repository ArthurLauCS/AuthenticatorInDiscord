//! 二维码图片识别。一张图里可以有多个码（比如一张截图里拼了好几张导出码），全部读出来。

/// 识别 PNG / JPEG / WebP / BMP 图片里的二维码，返回每个码的文字内容；图片读不了返回 None
pub fn decode_image(bytes: &[u8]) -> Option<Vec<String>> {
    let img = image::load_from_memory(bytes).ok()?.to_luma8();
    Some(decode_gray(img.width() as usize, img.height() as usize, img.as_raw()))
}

/// 识别 8 位灰度像素（逐行，每行 w 个）里的二维码
pub fn decode_gray(w: usize, h: usize, px: &[u8]) -> Vec<String> {
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| px[y * w + x]);
    img.detect_grids().into_iter().filter_map(|g| g.decode().ok().map(|(_, s)| s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// 生成两张码拼在一张 PNG 里（每格 4 像素、四周留白），识别后应读回两条原文
    #[test]
    fn two_codes_in_one_png() {
        let texts = ["otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZTAC", "otpauth://totp/bob?secret=JBSWY3DPEHPK3PXP"];
        let codes: Vec<_> = texts.iter().map(|t| qrcode::QrCode::new(t).unwrap()).collect();
        let (cell, quiet) = (4, 4);
        let side = codes.iter().map(|c| c.width()).max().unwrap() + 2 * quiet;
        let (w, h) = (side * cell * 2, side * cell);
        let mut img = image::GrayImage::from_pixel(w as u32, h as u32, image::Luma([255]));
        for (k, c) in codes.iter().enumerate() {
            let colors = c.to_colors();
            for (i, col) in colors.iter().enumerate() {
                if *col == qrcode::Color::Dark {
                    let (x, y) = ((i % c.width() + quiet) * cell + k * side * cell, (i / c.width() + quiet) * cell);
                    for dy in 0..cell {
                        for dx in 0..cell {
                            img.put_pixel((x + dx) as u32, (y + dy) as u32, image::Luma([0]));
                        }
                    }
                }
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut got = decode_image(&png).unwrap();
        got.sort();
        let mut want: Vec<String> = texts.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want);
        assert!(decode_image(b"not an image").is_none());
    }
}

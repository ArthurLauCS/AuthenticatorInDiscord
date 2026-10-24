//! 设置：%APPDATA%\AuthTile\config.txt，每行 键|值。界面语言也存这里。

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use windows::Win32::Globalization::GetUserDefaultUILanguage;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// 窗口左上角和宽度（物理像素）；宽度为 0 表示还没摆过
    pub pos: [i32; 3],
    pub topmost: bool,
    /// None 时跟随系统界面语言
    pub en: Option<bool>,
    pub hidden: bool,
    /// 磁贴底色的不透明度（20～100）
    pub opacity: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config { pos: [0; 3], topmost: false, en: None, hidden: false, opacity: 100 }
    }
}

impl Config {
    pub fn parse(s: &str) -> Config {
        let mut c = Config::default();
        for l in s.trim_start_matches('\u{feff}').lines() {
            let Some((k, v)) = l.split_once('|') else { continue };
            match k {
                "pos" => {
                    let n: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                    if let [x, y, w] = n[..] {
                        c.pos = [x, y, w];
                    }
                }
                "topmost" => c.topmost = v == "1",
                "lang" => c.en = Some(v == "en"),
                "hidden" => c.hidden = v == "1",
                "opacity" => c.opacity = v.parse().unwrap_or(100),
                _ => {}
            }
        }
        c
    }

    pub fn to_text(&self) -> String {
        let b = |v: bool| if v { "1" } else { "0" };
        let mut s = format!("pos|{},{},{}\ntopmost|{}\nhidden|{}\nopacity|{}\n", self.pos[0], self.pos[1], self.pos[2], b(self.topmost), b(self.hidden), self.opacity);
        if let Some(en) = self.en {
            s += if en { "lang|en\n" } else { "lang|zh\n" };
        }
        s
    }
}

static EN: AtomicBool = AtomicBool::new(false);

pub fn set_lang(en: Option<bool>) {
    // 主语言号在低 10 位，0x04 是中文
    EN.store(en.unwrap_or_else(|| (unsafe { GetUserDefaultUILanguage() } & 0x3FF) != 0x04), Relaxed);
}

pub fn en() -> bool {
    EN.load(Relaxed)
}

pub fn tr(zh: &'static str, en: &'static str) -> &'static str {
    if self::en() { en } else { zh }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let c = Config { pos: [-10, 20, 300], topmost: true, en: Some(true), hidden: true, opacity: 45 };
        assert_eq!(Config::parse(&c.to_text()), c);
        assert_eq!(Config::parse("\u{feff}pos|1,2\nbad\nlang|zh"), Config { en: Some(false), ..Default::default() });
    }
}

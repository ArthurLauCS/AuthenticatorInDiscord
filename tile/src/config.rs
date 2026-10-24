//! 设置：%APPDATA%\AuthTile\config.txt，每行 键|值。界面语言也存这里。

use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows_core::{PCWSTR, w};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// 窗口左上角和宽度（物理像素）；宽度为 0 表示还没摆过
    pub pos: [i32; 3],
    pub topmost: bool,
    /// None 时跟随系统界面语言
    pub lang: Option<Lang>,
    pub hidden: bool,
    /// 磁贴底色的不透明度（20～100）
    pub opacity: u32,
}

impl Default for Config {
    fn default() -> Config {
        Config { pos: [0; 3], topmost: false, lang: None, hidden: false, opacity: 100 }
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
                "lang" => c.lang = Lang::parse(v),
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
        if let Some(l) = self.lang {
            s += &format!("lang|{}\n", l.code());
        }
        s
    }
}

/// 界面语言：简体中文、繁体中文、英文
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lang {
    Zh,
    Tw,
    En,
}

impl Lang {
    /// config.txt 里的写法，也是安装包的语言名和 --lang 参数
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::Tw => "tw",
            Lang::En => "en",
        }
    }

    pub fn parse(s: &str) -> Option<Lang> {
        [Lang::Zh, Lang::Tw, Lang::En].into_iter().find(|l| l.code() == s)
    }
}

static LANG: AtomicU8 = AtomicU8::new(Lang::Zh as u8);

pub fn set_lang(lang: Option<Lang>) {
    let l = lang.unwrap_or_else(|| {
        // 主语言号在低 10 位，0x04 是中文；台湾、香港、澳门和中文（繁体）用繁体
        let id = unsafe { GetUserDefaultUILanguage() };
        match (id & 0x3FF, id) {
            (0x04, 0x0404 | 0x0C04 | 0x1404 | 0x7C04) => Lang::Tw,
            (0x04, _) => Lang::Zh,
            _ => Lang::En,
        }
    });
    LANG.store(l as u8, Relaxed);
}

pub fn lang() -> Lang {
    match LANG.load(Relaxed) {
        1 => Lang::Tw,
        2 => Lang::En,
        _ => Lang::Zh,
    }
}

pub fn tr(zh: &'static str, tw: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Zh => zh,
        Lang::Tw => tw,
        Lang::En => en,
    }
}

/// 文字排版的区域：繁体界面用 zh-TW，汉字取繁体字形；英文界面也按 zh-CN，因为账号名常是中文
pub fn locale() -> PCWSTR {
    if lang() == Lang::Tw { w!("zh-TW") } else { w!("zh-CN") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let c = Config { pos: [-10, 20, 300], topmost: true, lang: Some(Lang::Tw), hidden: true, opacity: 45 };
        assert_eq!(Config::parse(&c.to_text()), c);
        assert_eq!(Config::parse("\u{feff}pos|1,2\nbad\nlang|zh"), Config { lang: Some(Lang::Zh), ..Default::default() });
        assert_eq!(Config::parse("lang|xx").lang, None);
    }
}

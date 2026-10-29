//! Swiss 风格的全部取值：配色、字号、间距、字体名。视图代码只引用这里的名字，不写十六进制色值和裸数字。
//! 调色板来自 ui-style 目录的 swiss（黑白灰），强调色、强调色上的字用本程序的青绿（teal）。
//! 选中 = 反色：底用 text、字用 bg，所以每个主题同时带着另一种模式的调色板（inv）给选中行用。

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

// 字号（DIP）：标签 12、正文 13、小节 16、验证码 24；只用 400 和 700 两种字重
pub const S_LABEL: f32 = 12.0;
pub const S_BODY: f32 = 13.0;
pub const S_SECTION: f32 = 16.0;
pub const S_DISPLAY: f32 = 24.0;
/// 图标字体（Segoe Fluent Icons）只用一个字号
pub const S_ICON: f32 = 14.0;

// 间距：4 px 为单位，8 px 栅格
pub const U: f32 = 4.0;
/// 磁贴左右边距，和所有列的左边线
pub const PAD: f32 = 16.0;
/// 粗线（text 色）和细线（border 色）
pub const RULE: f32 = 2.0;
pub const HAIR: f32 = 1.0;
/// 行左边的结构线宽
pub const BAR: f32 = 2.0;
/// 倒计时圆环：半径和线宽
pub const RING_R: f32 = 11.0;
pub const RING_W: f32 = 2.0;

/// 主字体：TeX Gyre Heros（随程序带，fonts 文件夹），缺文件时退到 swiss 规定的 Arial
pub const FONT_LATIN: &str = "TeX Gyre Heros";
/// GDI（编辑框）看到的家族名是字体文件 name 表第 1 项，没有空格
pub const FONT_LATIN_GDI: &str = "TeXGyreHeros";
pub const FONT_LATIN_FALLBACK: &str = "Arial";
pub const FONT_FILES: [&str; 2] = ["texgyreheros-regular.otf", "texgyreheros-bold.otf"];
/// 汉字：简体界面用微软雅黑 UI，繁体界面用微软正黑体 UI
pub const FONT_CJK_ZH: &str = "Microsoft YaHei UI";
pub const FONT_CJK_TW: &str = "Microsoft JhengHei UI";
pub const FONT_ICON: &str = "Segoe Fluent Icons";
pub const FONT_ICON_OLD: &str = "Segoe MDL2 Assets";

fn hex(c: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((c >> 16) & 255) as f32 / 255.0, g: ((c >> 8) & 255) as f32 / 255.0, b: (c & 255) as f32 / 255.0, a: 1.0 }
}

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub fn with_a(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

/// 一种模式的一组颜色
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: D2D1_COLOR_F,
    pub surface: D2D1_COLOR_F,
    pub border: D2D1_COLOR_F,
    pub text: D2D1_COLOR_F,
    pub muted: D2D1_COLOR_F,
    pub accent: D2D1_COLOR_F,
    pub on_accent: D2D1_COLOR_F,
    /// 状态色：只用在“快过期”这一种状态上
    pub red: D2D1_COLOR_F,
}

impl Palette {
    pub fn new(dark: bool) -> Palette {
        if dark {
            Palette { bg: hex(0x0a0a0a), surface: hex(0x161616), border: hex(0x3a3a3a), text: hex(0xf5f5f5), muted: hex(0xa3a3a3), accent: hex(0x3cc4b4), on_accent: hex(0x0b1a19), red: hex(0xff5a4f) }
        } else {
            Palette { bg: hex(0xffffff), surface: hex(0xf2f2f2), border: hex(0xc8c8c8), text: hex(0x0a0a0a), muted: hex(0x595959), accent: hex(0x0b7068), on_accent: hex(0xffffff), red: hex(0xc4001a) }
        }
    }
}

pub struct Theme {
    pub dark: bool,
    /// 当前模式的颜色
    pub p: Palette,
    /// 另一种模式的颜色：选中行（反色）里的文字、强调色、状态色取这一套
    pub inv: Palette,
    /// 磁贴底色：bg 乘上透明度（0.2～1）
    pub tint: D2D1_COLOR_F,
    /// 其余填充（悬停的 surface、选中的反色块）的不透明度：不低于 0.6，免得透明度调低后看不见
    pub fill_a: f32,
    /// 底色太薄时文字压在壁纸上，加阴影（和桌面图标一样）
    pub shadow: bool,
}

impl Theme {
    /// opacity 是底色的不透明度（0.2～1）
    pub fn new(dark: bool, opacity: f32) -> Theme {
        let p = Palette::new(dark);
        Theme { dark, p, inv: Palette::new(!dark), tint: with_a(p.bg, opacity), fill_a: opacity.max(0.6), shadow: opacity < 0.5 }
    }

    /// 右键菜单、改名框用的不透明面板：不跟磁贴的透明度走
    pub fn panel(dark: bool) -> Theme {
        Theme::new(dark, 1.0)
    }

    pub fn surface(&self) -> D2D1_COLOR_F {
        with_a(self.p.surface, self.fill_a)
    }

    /// 选中（反色）块的填充
    pub fn selected(&self) -> D2D1_COLOR_F {
        with_a(self.p.text, self.fill_a)
    }

    /// 选中块里的字、弱化字、强调色、状态色：用另一种模式的调色板，在 text 色的底上读得清
    pub fn on_selected(&self) -> Palette {
        Palette { text: self.p.bg, ..self.inv }
    }
}

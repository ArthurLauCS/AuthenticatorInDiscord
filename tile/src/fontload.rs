//! 随程序带的字体：exe 旁的 fonts 文件夹（开发时是 tile\assets\fonts），由 scripts/fonts.ps1 取得。
//! 做成 DirectWrite 字体集合（GDI 的 FR_PRIVATE 字体 DirectWrite 看不到）；编辑框用的 GDI 字体另外私有加载同一份文件。
//! 任何一步失败都返回 None，调用方退回系统字体，不报错。

use std::path::PathBuf;

use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::{AddFontResourceExW, FR_PRIVATE};
use windows::core::*;

use crate::config::{Lang, lang};
use crate::theme::{FONT_CJK_TW, FONT_CJK_ZH, FONT_FILES};

pub struct Bundled {
    /// TeX Gyre Heros 常规体 + 粗体
    pub latin: Option<IDWriteFontCollection>,
    /// 汉字（和 Heros 没有的符号）落到界面语言对应的系统黑体，再交给系统的后备链，不显示方框
    pub fallback: Option<IDWriteFontFallback>,
    /// GDI 里能不能用 TeXGyreHeros（编辑框）
    pub gdi: bool,
}

/// 字体文件夹：先看 exe 旁的 fonts，再看开发布局 target\<profile>\authtile.exe 往上两级的 tile\assets\fonts
fn font_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("fonts"), dir.join("..").join("..").join("tile").join("assets").join("fonts")].into_iter().find(|d| FONT_FILES.iter().all(|f| d.join(f).is_file()))
}

fn collection(dw3: &IDWriteFactory3, dir: &std::path::Path) -> Option<IDWriteFontCollection> {
    unsafe {
        let builder = dw3.CreateFontSetBuilder().ok()?;
        for f in FONT_FILES {
            let wide: Vec<u16> = dir.join(f).as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            let face = dw3.CreateFontFaceReference2(PCWSTR(wide.as_ptr()), None, 0, DWRITE_FONT_SIMULATIONS_NONE).ok()?;
            builder.AddFontFaceReference2(&face).ok()?;
        }
        let set = builder.CreateFontSet().ok()?;
        dw3.CreateFontCollectionFromFontSet(&set).ok()?.cast().ok()
    }
}

fn fallback(dw: &IDWriteFactory) -> Option<IDWriteFontFallback> {
    unsafe {
        let dw2: IDWriteFactory2 = dw.cast().ok()?;
        let b = dw2.CreateFontFallbackBuilder().ok()?;
        // 汉字、部首、标点、全角、兼容区、扩展 B 及以后
        let ranges = [(0x2E80, 0x2FDF), (0x3000, 0x303F), (0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0xFF00, 0xFFEF), (0x20000, 0x2FA1F)].map(|(first, last)| DWRITE_UNICODE_RANGE { first, last });
        let cjk = if lang() == Lang::Tw { FONT_CJK_TW } else { FONT_CJK_ZH };
        let name: Vec<u16> = cjk.encode_utf16().chain([0]).collect();
        b.AddMapping(&ranges, &[name.as_ptr()], None, None, PCWSTR::null(), 1.0).ok()?;
        if let Ok(sys) = dw2.GetSystemFontFallback() {
            let _ = b.AddMappings(&sys);
        }
        b.CreateFontFallback().ok()
    }
}

impl Bundled {
    pub fn load(dw: &IDWriteFactory) -> Bundled {
        let fallback = fallback(dw);
        let (Some(dir), Ok(dw3)) = (font_dir(), dw.cast::<IDWriteFactory3>()) else { return Bundled { latin: None, fallback, gdi: false } };
        // 只给本进程用的 GDI 字体（编辑框）；重复加载只是引用计数加一
        let gdi = FONT_FILES.iter().all(|f| {
            let wide: Vec<u16> = dir.join(f).as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            unsafe { AddFontResourceExW(PCWSTR(wide.as_ptr()), FR_PRIVATE, None) > 0 }
        });
        Bundled { latin: collection(&dw3, &dir), fallback, gdi }
    }
}

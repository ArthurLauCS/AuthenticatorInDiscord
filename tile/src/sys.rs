//! 系统接口的薄封装：开机自启、注册表、DPAPI 加解密、剪贴板、打开文件对话框、拖放的文件。

use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::Security::Cryptography::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::*;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("AuthTile");

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

pub fn exe_path() -> String {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

pub fn autostart() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if on {
            let cmd = wide(&format!("\"{}\"", exe_path()));
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, REG_SZ.0, Some(cmd.as_ptr() as _), (cmd.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME);
        }
    }
}

pub fn reg_dword(key: PCWSTR, name: PCWSTR) -> Option<u32> {
    let (mut v, mut n) = (0u32, 4u32);
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as _), Some(&mut n)) };
    (r == ERROR_SUCCESS).then_some(v)
}

/// DPAPI：按当前 Windows 用户加密，换电脑或换用户解不开
pub fn protect(data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let r = if encrypt {
            CryptProtectData(&input, w!("AuthTile"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        r.ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        Some(v)
    }
}

/// 把验证码放进剪贴板，并标记不进剪贴板历史（Win+V）和云同步
pub fn copy_text(hwnd: HWND, s: &str) -> bool {
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let w = wide(s);
        let ok = global(bytemuck(&w)).is_some_and(|h| SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(h.0))).is_ok());
        let ex = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        if let Some(h) = global(&[0]) {
            let _ = SetClipboardData(ex, Some(HANDLE(h.0)));
        }
        let _ = CloseClipboard();
        ok
    }
}

fn bytemuck(w: &[u16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2) }
}

fn global(b: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, b.len()).ok()?;
        let p = GlobalLock(h) as *mut u8;
        std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
        let _ = GlobalUnlock(h);
        Some(h)
    }
}

/// 剪贴板里能导入的东西
pub enum Clip {
    /// 图片文件内容（PNG，或补上文件头的 BMP）
    Image(Vec<u8>),
    Files(Vec<PathBuf>),
    Text(String),
    None,
}

pub fn read_clip(hwnd: HWND) -> Clip {
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return Clip::None;
        }
        let get = |fmt: u32| -> Option<Vec<u8>> {
            let h = GetClipboardData(fmt).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u8;
            if p.is_null() {
                return None;
            }
            let v = std::slice::from_raw_parts(p, GlobalSize(g)).to_vec();
            let _ = GlobalUnlock(g);
            Some(v)
        };
        // 截图工具、浏览器会同时放 PNG 和 DIB，PNG 无损且带尺寸信息，优先用
        let png = RegisterClipboardFormatW(w!("PNG"));
        let c = if let Some(b) = get(png) {
            Clip::Image(b)
        } else if let Some(d) = get(CF_DIB.0 as u32) {
            Clip::Image(dib_to_bmp(&d))
        } else if let Ok(h) = GetClipboardData(CF_HDROP.0 as u32) {
            Clip::Files(drop_files(HDROP(h.0)))
        } else if let Some(t) = get(CF_UNICODETEXT.0 as u32) {
            let w: Vec<u16> = t.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
            Clip::Text(String::from_utf16_lossy(&w))
        } else {
            Clip::None
        };
        let _ = CloseClipboard();
        c
    }
}

/// 剪贴板的 DIB 就是 BMP 文件去掉了 14 字节文件头，补回去交给图片解码
fn dib_to_bmp(d: &[u8]) -> Vec<u8> {
    let u32_at = |i: usize| d.get(i..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (size, bits, comp, used) = (u32_at(0), d.get(14).copied().unwrap_or(0) as u32, u32_at(16), u32_at(32));
    let colors = if used != 0 { used } else if bits <= 8 { 1 << bits } else { 0 };
    // BI_BITFIELDS 且是 40 字节的旧信息头时，后面跟 3 个颜色掩码
    let masks = if comp == 3 && size == 40 { 12 } else { 0 };
    let off = 14 + size + masks + colors * 4;
    let mut b = Vec::with_capacity(14 + d.len());
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&((14 + d.len()) as u32).to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&off.to_le_bytes());
    b.extend_from_slice(d);
    b
}

pub fn drop_files(h: HDROP) -> Vec<PathBuf> {
    unsafe {
        let n = DragQueryFileW(h, u32::MAX, None);
        (0..n)
            .map(|i| {
                let mut buf = vec![0u16; DragQueryFileW(h, i, None) as usize + 1];
                let len = DragQueryFileW(h, i, Some(&mut buf)) as usize;
                PathBuf::from(String::from_utf16_lossy(&buf[..len]))
            })
            .collect()
    }
}

/// 系统的打开文件对话框，可多选图片
pub fn open_images(hwnd: HWND, title: &str, filter_name: &str) -> Vec<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let filter: Vec<u16> = format!("{filter_name}\0*.png;*.jpg;*.jpeg;*.webp;*.bmp\0\0").encode_utf16().collect();
    let title = wide(title);
    let mut ofn = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_ALLOWMULTISELECT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return Vec::new();
    }
    // 单选：完整路径；多选：目录 \0 文件名 \0 文件名 \0\0
    let parts: Vec<String> = buf.split(|&c| c == 0).take_while(|s| !s.is_empty()).map(String::from_utf16_lossy).collect();
    match &parts[..] {
        [one] => vec![PathBuf::from(one)],
        [dir, names @ ..] => names.iter().map(|n| PathBuf::from(dir).join(n)).collect(),
        [] => Vec::new(),
    }
}

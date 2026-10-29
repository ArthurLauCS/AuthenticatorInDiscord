#![windows_subsystem = "windows"]
//! 验证码磁贴：桌面上一块小窗，显示导入的 TOTP 验证码，每秒刷新，点一下复制。
//! 导入 Google 验证器导出的二维码图片（拖进来、选文件、Ctrl+V 粘贴截图），也认 otpauth:// 链接；已有的账号自动跳过。
//! 数据用 DPAPI 按当前 Windows 用户加密，存在 %APPDATA%\AuthTile\vault.dat。
//! 验证码可以绑定台服 Lost Ark 的账号密码，行上的登录按钮一键登录启动器（launcher.rs）。
//!
//! 参数：--data <目录> 代替 %APPDATA%\AuthTile；--top 置顶；--light / --dark 强制主题；--lang zh|tw|en 强制语言（后几个是截图走查用的）。

mod config;
mod fontload;
mod gfx;
mod launcher;
mod menu;
mod sys;
mod theme;
mod view;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use authcore::Vault;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{EM_SETSEL, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use config::{Config, Lang, tr};
use sys::wide;
use theme::Theme;
use view::{Fonts, Hit};

const CLASS: PCWSTR = w!("AuthTileMain");
const WM_APP_TRAY: u32 = WM_APP + 1;
/// 后台识别完一批图片
const WM_APP_SCAN: u32 = WM_APP + 2;
/// 菜单关掉之后再开改名框（wparam = 条目编号，lparam = Field），免得两个窗口抢焦点
const WM_APP_RENAME: u32 = WM_APP + 3;
/// 后台登录启动器结束（lparam 见 launcher）
const WM_APP_LOGIN: u32 = WM_APP + 4;
const T_TICK: usize = 1;
const T_COPIED: usize = 2;
/// 拖动排序时贴着列表边缘自动滚动
const T_DRAG: usize = 3;
const EDGE: f32 = 6.0;
const DEFAULT_W: f32 = 300.0;
/// 导入结果提示显示多久（毫秒）
const NOTE_MS: u64 = 6000;

/// 后台读图、识别二维码的结果
struct Scan {
    /// 打不开的图片数
    bad: usize,
    /// 识别到的二维码内容（剪贴板里的文字也算）
    uris: Vec<String>,
}

/// 要导入的东西
enum Src {
    Files(Vec<PathBuf>),
    Image(Vec<u8>),
    /// 复制来的链接文字，每行一条
    Text(String),
}

/// 改名框在改哪一项
#[derive(Clone, Copy, PartialEq)]
pub enum Field {
    Label,
    User,
    Pass,
}

pub struct App {
    data: PathBuf,
    pub cfg: Config,
    hwnd: HWND,
    pub d2d: ID2D1Factory1,
    pub gfx: gfx::Gfx,
    pub dw: IDWriteFactory,
    pub fonts: Fonts,
    pub theme: Theme,
    force_dark: Option<bool>,
    surf: Option<gfx::Surface>,
    /// 系统主题是不是深色（菜单跟它走；磁贴在低透明度时会强制深色）
    pub sys_dark: bool,
    pub menu: Option<Box<menu::Menu>>,
    /// 当前菜单是哪一条验证码的（None = 设置菜单）
    pub menu_for: Option<u32>,
    pub vault: Vault,
    /// vault.dat 解不开（换了电脑或 Windows 用户）：不再写盘，免得把原文件覆盖掉
    pub locked: bool,
    /// 提示文字、是不是错误、显示到什么时候（GetTickCount64）
    note: Option<(String, bool, u64)>,
    /// 刚复制过的条目，显示“已复制”
    pub copied: Option<u32>,
    /// 正在改的条目、哪一项和改名框
    pub editing: Option<(u32, Field)>,
    edit: Option<HWND>,
    edit_brush: HBRUSH,
    pub hits: Vec<(D2D_RECT_F, Hit)>,
    pub hover: Option<Hit>,
    /// 列表滚动位置、列表区域 (顶, 可见高度, 总高)
    pub scroll: f32,
    pub list: (f32, f32, f32),
    /// 已隐藏的验证码展开着没有（不存盘，启动时收起）
    pub expanded: bool,
    pub drag: Option<view::Drag>,
    tracking: bool,
    /// 按下左键：屏幕坐标、窗口位置、是否已经在拖（拖窗口或拖排序）
    press: Option<(POINT, RECT, bool)>,
    /// 后台正在登录启动器，没结束前不再开一次
    logging_in: bool,
    tray_icon: HICON,
    taskbar_created: u32,
    rx: Receiver<Scan>,
    tx: Sender<Scan>,
}

static mut APP: *mut App = std::ptr::null_mut();

/// 单线程 UI，全局状态只在主线程上访问
pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn scale(hwnd: HWND) -> f32 {
    unsafe { GetDpiForWindow(hwnd) as f32 / 96.0 }
}

fn lparam_pt(lp: LPARAM) -> POINT {
    POINT { x: lp.0 as i16 as i32, y: (lp.0 >> 16) as i16 as i32 }
}

/// 鼠标消息的位置换成屏幕坐标。不用 GetCursorPos：测试脚本发来的鼠标消息也按消息里的位置拖
fn screen_pt(hwnd: HWND, lp: LPARAM) -> POINT {
    let mut p = lparam_pt(lp);
    unsafe {
        let _ = ClientToScreen(hwnd, &mut p);
    }
    p
}

fn copy_wide(dst: &mut [u16], s: &str) {
    let n = dst.len() - 1;
    for (d, c) in dst.iter_mut().take(n).zip(s.encode_utf16().chain([0])) {
        *d = c;
    }
}

fn work_area(mon: HMONITOR) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(mon, &mut mi);
    }
    mi.rcWork
}

/// 矩形整个挪进工作区，返回新的左上角；比工作区还大时贴左上
fn fit(r: &RECT, wa: RECT) -> (i32, i32) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    (r.left.min(wa.right - w).max(wa.left), r.top.min(wa.bottom - h).max(wa.top))
}

fn keep_on_screen(hwnd: HWND) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
        let (x, y) = fit(&r, work_area(MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST)));
        if (x, y) != (r.left, r.top) {
            let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// COLORREF 是 0x00BBGGRR
fn colorref(c: windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F) -> COLORREF {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(b(c.r) | b(c.g) << 8 | b(c.b) << 16)
}

/// 读图片、识别二维码（在后台线程里跑：手机拍的大图要几百毫秒）
fn scan(src: Src) -> Scan {
    let images = match src {
        Src::Image(b) => vec![Some(b)],
        Src::Files(ps) => ps.iter().map(|p| std::fs::read(p).ok()).collect(),
        Src::Text(t) => return Scan { bad: 0, uris: t.lines().map(str::to_string).collect() },
    };
    let mut s = Scan { bad: 0, uris: Vec::new() };
    for img in images {
        match img.as_deref().and_then(authcore::decode_image) {
            Some(u) => s.uris.extend(u),
            None => s.bad += 1,
        }
    }
    s
}

impl App {
    fn cfg_path(&self) -> PathBuf {
        self.data.join("config.txt")
    }

    fn vault_path(&self) -> PathBuf {
        self.data.join("vault.dat")
    }

    fn save_cfg(&self) {
        let _ = std::fs::create_dir_all(&self.data);
        let _ = std::fs::write(self.cfg_path(), self.cfg.to_text());
    }

    /// 加密后先写临时文件再改名替换，写到一半断电也不会坏掉原文件
    fn save_vault(&mut self) {
        if self.locked {
            return;
        }
        let ok = sys::protect(self.vault.to_text().as_bytes(), true).is_some_and(|enc| {
            let tmp = self.data.join("vault.tmp");
            std::fs::create_dir_all(&self.data).is_ok() && std::fs::write(&tmp, enc).is_ok() && std::fs::rename(&tmp, self.vault_path()).is_ok()
        });
        if !ok {
            self.set_note(tr("保存失败，改动在重启后会丢失", "儲存失敗，變更在重新啟動後會遺失", "Could not save; changes will be lost on restart").into(), true);
        }
    }

    pub fn note(&self) -> Option<(&str, bool)> {
        self.note.as_ref().filter(|n| unsafe { GetTickCount64() } < n.2).map(|n| (n.0.as_str(), n.1))
    }

    fn set_note(&mut self, msg: String, err: bool) {
        self.note = Some((msg, err, unsafe { GetTickCount64() } + NOTE_MS));
        self.invalidate();
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn load_theme(&mut self) {
        self.sys_dark = self.force_dark.unwrap_or_else(|| sys::reg_dword(w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w!("AppsUseLightTheme")) != Some(1));
        // 透明度低于 50% 时底色太薄，深色字在深色壁纸上看不清：不管系统主题，都用桌面图标那样的白字加阴影
        let o = self.cfg.opacity.clamp(menu::OPACITY_MIN, 100) as f32 / 100.0;
        let dark = self.sys_dark || o < 0.5;
        self.theme = Theme::new(dark, o);
        unsafe {
            let d = BOOL::from(dark);
            let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &d as *const _ as _, 4);
            let corner = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as _, 4);
            // 改名框的配色
            let _ = DeleteObject(self.edit_brush.into());
            self.edit_brush = CreateSolidBrush(colorref(Theme::panel(dark).tint));
        }
        self.invalidate();
    }

    fn paint(&mut self) {
        unsafe {
            let _ = ValidateRect(Some(self.hwnd), None);
            let mut rc = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            if self.surf.is_none() {
                self.surf = self.gfx.surface(self.hwnd, rc.right as u32, rc.bottom as u32).ok();
            }
            let Some(surf) = self.surf.clone() else { return };
            let s = scale(self.hwnd);
            if self.gfx.bind(&surf, s * 96.0).is_err() {
                return self.reset_gfx();
            }
            let mut wr = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut wr);
            let wa = work_area(MonitorFromRect(&wr, MONITOR_DEFAULTTONEAREST));
            let max_h = (wa.bottom - wa.top) as f32 / s;
            self.gfx.dc.BeginDraw();
            let fr = view::draw(self, rc.right as f32 / s, rc.bottom as f32 / s, max_h, now_ms() / 1000);
            if self.gfx.dc.EndDraw(None, None).is_err() {
                return self.reset_gfx();
            }
            let _ = self.gfx.present(&surf);
            self.hits = fr.hits;
            self.list = fr.list;
            self.scroll = self.scroll.min((fr.list.2 - fr.list.1).max(0.0));
            // 高度跟着条目数走，最高到工作区高。顶边以用户摆的位置为准，放不下才临时往上挪；拖动窗口途中不动位置
            let want = (fr.h.min(max_h) * s).ceil() as i32;
            if want != rc.bottom {
                let (x, y) = fit(&RECT { left: wr.left, top: self.cfg.pos[1], right: wr.right, bottom: self.cfg.pos[1] + want }, wa);
                let flags = if self.press.is_some() { SWP_NOMOVE } else { SET_WINDOW_POS_FLAGS(0) };
                let _ = SetWindowPos(self.hwnd, None, x, y, rc.right, want, flags | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }

    /// 设备丢失（驱动更新、显卡重置）后重建
    fn reset_gfx(&mut self) {
        if let Ok(g) = gfx::Gfx::new(&self.d2d) {
            self.gfx = g;
            self.surf = None;
            self.invalidate();
        }
    }

    fn hit_at(&self, pt: POINT) -> Option<Hit> {
        let s = scale(self.hwnd);
        let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
        self.hits.iter().find(|(r, _)| view::inside(r, x, y)).map(|(_, h)| *h)
    }

    fn click(&mut self, h: Hit) {
        match h {
            Hit::Row(id) => self.copy(id),
            Hit::Login(id) => self.login(id),
            Hit::Import => self.import_dialog(),
            Hit::Pin => self.set_topmost(!self.cfg.topmost),
            Hit::More => self.expanded = !self.expanded,
        }
        self.invalidate();
    }

    /// 按住拖过了阈值：按在标题栏空白处拖窗口，按在验证码上拖排序；按在按钮上不拖，松开时照常算点击
    fn start_drag(&mut self, p0: POINT) {
        let mut c = p0;
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut c);
        }
        let y = c.y as f32 / scale(self.hwnd);
        let moved = match self.hit_at(c) {
            Some(Hit::Row(id)) => {
                let lay = view::layout(self);
                let Some(&(_, top)) = lay.rows.iter().find(|(i, _)| self.vault.entries[*i].id == id) else { return };
                self.finish_rename(true);
                self.drag = Some(view::Drag { id, grab: y - self.list.0 + self.scroll - top, y });
                unsafe {
                    SetTimer(Some(self.hwnd), T_DRAG, 16, None);
                }
                true
            }
            None => y < view::HEAD,
            _ => false,
        };
        if let Some(p) = self.press.as_mut() {
            p.2 = moved;
        }
    }

    /// 拖动排序时鼠标贴着列表上下边缘，列表往那边滚
    fn drag_scroll(&mut self) {
        let Some(d) = &self.drag else { return };
        let (top, vis, h) = self.list;
        let v = if d.y < top + view::ROW / 2.0 {
            -8.0
        } else if d.y > top + vis - view::ROW / 2.0 {
            8.0
        } else {
            return;
        };
        let s = (self.scroll + v).clamp(0.0, (h - vis).max(0.0));
        if s != self.scroll {
            self.scroll = s;
            self.invalidate();
        }
    }

    /// 松开左键：拖排序的放下、拖窗口的记下位置、没拖过的算点击。鼠标被别的窗口抢走（弹窗、Alt+Tab）时 at 为 None，当作在原处松开
    fn release(&mut self, press: Option<(POINT, RECT, bool)>, at: Option<POINT>) {
        let Some((_, _, moved)) = press else { return };
        if let Some(id) = self.drag.as_ref().map(|d| d.id) {
            let slot = view::layout(self).slot;
            self.drag = None;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), T_DRAG);
            }
            if self.vault.place(id, slot) {
                self.save_vault();
            }
            self.invalidate();
        } else if moved {
            self.save_pos();
        } else if let Some(h) = at.and_then(|p| self.hit_at(p)) {
            self.click(h);
        }
    }

    fn copy(&mut self, id: u32) {
        let Some(e) = self.vault.get(id) else { return };
        if sys::copy_text(self.hwnd, &e.otp.code(now_ms() / 1000)) {
            self.copied = Some(id);
            unsafe {
                SetTimer(Some(self.hwnd), T_COPIED, 1500, None);
            }
        }
        self.invalidate();
    }

    fn login(&mut self, id: u32) {
        let Some(e) = self.vault.get(id).filter(|e| e.can_login()) else { return };
        if self.logging_in {
            return self.set_note(tr("正在登录，请稍候", "正在登入，請稍候", "Signing in, please wait").into(), false);
        }
        let msg = tr("正在登录 {0}…", "正在登入 {0}…", "Signing in {0}…").replace("{0}", &e.label);
        launcher::start(self.hwnd, e.user.clone(), e.pass.clone(), e.otp.clone());
        self.logging_in = true;
        self.set_note(msg, false);
    }

    fn import_dialog(&mut self) {
        let files = sys::open_images(self.hwnd, tr("选择验证器导出的二维码图片", "選擇驗證器匯出的 QR 碼圖片", "Choose exported QR code images"), tr("图片", "圖片", "Images"));
        if !files.is_empty() {
            self.start_scan(Src::Files(files));
        }
    }

    fn import_clip(&mut self) {
        match sys::read_clip(self.hwnd) {
            sys::Clip::Image(b) => self.start_scan(Src::Image(b)),
            sys::Clip::Files(f) => self.start_scan(Src::Files(f)),
            sys::Clip::Text(t) => self.start_scan(Src::Text(t)),
            sys::Clip::None => self.set_note(tr("剪贴板里没有图片", "剪貼簿裡沒有圖片", "No image on the clipboard").into(), true),
        }
    }

    fn start_scan(&mut self, src: Src) {
        // 数据解不开时不导入，也不盖掉常驻的出错提示
        if self.locked {
            return;
        }
        self.set_note(tr("正在识别二维码…", "正在辨識 QR 碼…", "Reading QR codes…").into(), false);
        let (tx, hw) = (self.tx.clone(), self.hwnd.0 as usize);
        std::thread::spawn(move || {
            let _ = tx.send(scan(src));
            unsafe {
                let _ = PostMessageW(Some(HWND(hw as _)), WM_APP_SCAN, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn on_scan(&mut self, s: Scan) {
        let Some((added, dup, unsupported)) = self.vault.import_uris(&s.uris) else {
            let msg = match (s.uris.is_empty(), s.bad > 0) {
                (true, true) => tr("图片打不开，请用 PNG、JPG、WebP 或 BMP", "無法開啟圖片，請使用 PNG、JPG、WebP 或 BMP", "Can't open the image; use PNG, JPG, WebP or BMP"),
                (true, false) => tr("没有识别到二维码，请截得清晰、完整一些", "沒有辨識到 QR 碼，請擷取清晰、完整的畫面", "No QR code found; try a sharper, complete shot"),
                (false, _) => tr("这不是验证器的二维码", "這不是驗證器的 QR 碼", "Not an authenticator QR code"),
            };
            return self.set_note(msg.into(), true);
        };
        if added > 0 {
            self.save_vault();
        }
        let mut msg = tr("导入 {0} 个，{1} 个已存在", "匯入 {0} 個，{1} 個已存在", "Imported {0}, {1} already here").replace("{0}", &added.to_string()).replace("{1}", &dup.to_string());
        if unsupported > 0 {
            msg += &tr("，{0} 个不支持（HOTP 等）", "，{0} 個不支援（HOTP 等）", ", {0} unsupported (HOTP etc.)").replace("{0}", &unsupported.to_string());
        }
        self.set_note(msg, added == 0 && unsupported > 0);
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.hwnd, uID: 1, uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP, uCallbackMessage: WM_APP_TRAY, hIcon: self.tray_icon, ..Default::default() };
        copy_wide(&mut nid.szTip, "Authenticator");
        nid
    }

    fn tray(&self, msg: NOTIFY_ICON_MESSAGE) {
        unsafe {
            let _ = Shell_NotifyIconW(msg, &self.tray_data());
        }
    }

    fn set_hidden(&mut self, hidden: bool) {
        self.cfg.hidden = hidden;
        self.save_cfg();
        unsafe {
            let _ = ShowWindow(self.hwnd, if hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
    }

    fn set_topmost(&mut self, on: bool) {
        self.cfg.topmost = on;
        self.save_cfg();
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    fn save_pos(&mut self) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        self.cfg.pos = [r.left, r.top, r.right - r.left];
        self.save_cfg();
    }

    /// 在鼠标处弹出右键菜单；row 是右键所在的验证码（None = 设置菜单，托盘也用它）
    fn menu(&mut self, row: Option<u32>) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        // 先关旧菜单（会清掉 menu_for），再记这次的行：菜单高度按行数算
        menu::close(self);
        self.menu_for = row;
        menu::show(self, pt);
        self.invalidate();
    }

    pub fn menu_closed(&mut self) {
        if self.menu_for.take().is_some() {
            self.invalidate();
        }
    }

    /// 菜单里点了某项；返回 true 表示菜单要关
    pub fn menu_action(&mut self, id: menu::Id) -> bool {
        use menu::Id;
        let row = self.menu_for;
        match id {
            Id::Opacity => return false,
            Id::Top => self.set_topmost(!self.cfg.topmost),
            Id::Auto => sys::set_autostart(!sys::autostart()),
            Id::Lang(l) => {
                self.cfg.lang = Some(l);
                config::set_lang(self.cfg.lang);
                // 字体的排版区域跟着语言走（繁体用 zh-TW 字形），重建一次
                unsafe {
                    let _ = DeleteObject(self.fonts.edit.into());
                }
                self.fonts = Fonts::new(&self.dw, unsafe { GetDpiForSystem() });
                self.save_cfg();
            }
            Id::ImportFile => {
                // 对话框是模态的，先让菜单关掉
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), WM_COMMAND, WPARAM(1), LPARAM(0));
                }
            }
            Id::ImportClip => self.import_clip(),
            Id::Hide => self.set_hidden(!self.cfg.hidden),
            Id::Exit => {
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
                return true;
            }
            Id::Copy => row.into_iter().for_each(|r| self.copy(r)),
            Id::Rename | Id::SetLogin => unsafe {
                if let Some(r) = row {
                    let f = if id == Id::Rename { Field::Label } else { Field::User };
                    let _ = PostMessageW(Some(self.hwnd), WM_APP_RENAME, WPARAM(r as usize), LPARAM(f as isize));
                }
            },
            Id::LoginBtn => {
                if row.is_some_and(|r| self.vault.set_login(r, !self.vault.get(r).is_some_and(|e| e.login))) {
                    self.save_vault();
                }
            }
            Id::Up | Id::Down => {
                if row.is_some_and(|r| self.vault.shift(r, if id == Id::Up { -1 } else { 1 })) {
                    self.save_vault();
                }
            }
            Id::HideRow => {
                if row.is_some_and(|r| self.vault.hide(r, !self.vault.get(r).is_some_and(|e| e.hidden))) {
                    self.save_vault();
                }
            }
            Id::Delete => {
                if row.is_some_and(|r| self.vault.remove(r)) {
                    self.save_vault();
                }
            }
        }
        self.invalidate();
        !matches!(id, Id::Top | Id::Auto | Id::Lang(_) | Id::Up | Id::Down | Id::LoginBtn)
    }

    /// 改透明度（拖滑块时实时生效，松手再存盘）
    pub fn set_opacity(&mut self, v: u32) {
        self.cfg.opacity = v;
        self.load_theme();
    }

    /// 改名框：无边框、颜色跟主题（WM_CTLCOLOREDIT 里设），盖在这一行的名字上；回车保存，Esc 取消，点别处也保存。
    /// 也用来填登录账号、密码（f）：密码框显示星号、打开时是空的
    fn start_rename(&mut self, id: u32, f: Field) {
        self.finish_rename(false);
        let Some(&(i, y)) = view::layout(self).rows.iter().find(|(i, _)| self.vault.entries[*i].id == id) else { return };
        let s = scale(self.hwnd);
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let r = view::label_rect(self, y, rc.right as f32 / s);
        let mut pt = POINT { x: (r.left * s) as i32, y: (r.top * s) as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut pt);
            let e = &self.vault.entries[i];
            let t = wide(match f {
                Field::Label => &e.label,
                Field::User => &e.user,
                Field::Pass => "",
            });
            let pw = if f == Field::Pass { ES_PASSWORD as u32 } else { 0 };
            let Ok(e) = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                w!("EDIT"),
                PCWSTR(t.as_ptr()),
                WS_POPUP | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32 | pw),
                pt.x,
                pt.y,
                ((r.right - r.left) * s) as i32,
                ((r.bottom - r.top) * s) as i32,
                Some(self.hwnd),
                None,
                None,
                None,
            ) else {
                return;
            };
            self.edit = Some(e);
            self.editing = Some((id, f));
            SendMessageW(e, WM_SETFONT, Some(WPARAM(self.fonts.edit.0 as usize)), Some(LPARAM(1)));
            SendMessageW(e, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            let _ = windows::Win32::UI::Shell::SetWindowSubclass(e, Some(edit_proc), 1, 0);
            let _ = SetForegroundWindow(e);
            let _ = SetFocus(Some(e));
        }
        self.invalidate();
    }

    fn finish_rename(&mut self, commit: bool) {
        let (Some(e), Some((id, f))) = (self.edit.take(), self.editing.take()) else { return };
        let mut buf = [0u16; 260];
        let n = unsafe { GetWindowTextW(e, &mut buf) } as usize;
        unsafe {
            let _ = DestroyWindow(e);
        }
        let t = String::from_utf16_lossy(&buf[..n]);
        let changed = commit
            && match f {
                Field::Label => self.vault.rename(id, &t),
                Field::User => self.vault.set_user(id, &t),
                Field::Pass => self.vault.set_pass(id, &t),
            };
        if changed {
            self.save_vault();
        }
        self.invalidate();
    }

    /// 每秒跳一次，对齐到整秒，换码那一刻立即重画
    fn tick(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), T_TICK, (1000 - now_ms() % 1000 + 15) as u32, None);
        }
        if !self.cfg.hidden {
            self.invalidate();
        }
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    unsafe {
        match msg {
            WM_KEYDOWN if wp.0 == VK_RETURN.0 as usize || wp.0 == VK_ESCAPE.0 as usize => {
                let a = app();
                let enter = wp.0 == VK_RETURN.0 as usize;
                // 账号填完回车接着填密码；账号清空了就不用填
                let next = a.editing.filter(|&(_, f)| enter && f == Field::User).map(|(id, _)| id);
                a.finish_rename(enter);
                if let Some(id) = next.filter(|&id| a.vault.get(id).is_some_and(|e| !e.user.is_empty())) {
                    let _ = PostMessageW(Some(a.hwnd), WM_APP_RENAME, WPARAM(id as usize), LPARAM(Field::Pass as isize));
                }
                LRESULT(0)
            }
            WM_CHAR if wp.0 == 13 || wp.0 == 27 => LRESULT(0),
            WM_KILLFOCUS => {
                app().finish_rename(true);
                LRESULT(0)
            }
            _ => DefSubclassProc(hwnd, msg, wp, lp),
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            // 整个窗口都是客户区，没有系统边框
            WM_NCCALCSIZE if wp.0 != 0 => return LRESULT(0),
            WM_ERASEBKGND => return LRESULT(1),
            _ => {}
        }
        if APP.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let a = app();
        match msg {
            WM_PAINT => a.paint(),
            WM_SIZE => {
                if let Some(s) = &a.surf {
                    a.gfx.resize(s, (lp.0 & 0xFFFF) as u32, ((lp.0 >> 16) & 0xFFFF) as u32);
                }
                a.invalidate();
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                a.surf = None;
                a.save_pos();
            }
            // 只能左右拖边改宽度，高度跟着内容
            WM_NCHITTEST => {
                let p = lparam_pt(lp);
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                let e = (EDGE * scale(hwnd)) as i32;
                let code = if p.x < r.left + e {
                    HTLEFT
                } else if p.x >= r.right - e {
                    HTRIGHT
                } else {
                    HTCLIENT
                };
                return LRESULT(code as isize);
            }
            WM_GETMINMAXINFO => {
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                mm.ptMinTrackSize.x = (view::MIN_W * scale(hwnd)) as i32;
            }
            WM_EXITSIZEMOVE => a.save_pos(),
            // 置顶时点磁贴不抢焦点；不置顶时和普通窗口一样，点了激活（这时可以 Ctrl+V 粘贴截图）。
            // 菜单开着时也不激活：否则菜单先因失活关掉，这次点击会落到磁贴的按钮上
            WM_MOUSEACTIVATE if a.cfg.topmost || a.menu.is_some() => return LRESULT(MA_NOACTIVATE as isize),
            WM_MOUSEMOVE => {
                if let Some((p0, r0, moved)) = a.press {
                    let p = screen_pt(hwnd, lp);
                    let (dx, dy) = (p.x - p0.x, p.y - p0.y);
                    if !moved && (dx.abs() > GetSystemMetrics(SM_CXDRAG) || dy.abs() > GetSystemMetrics(SM_CYDRAG)) {
                        a.start_drag(p0);
                    }
                    if let Some(d) = a.drag.as_mut() {
                        d.y = lparam_pt(lp).y as f32 / scale(hwnd);
                        a.invalidate();
                    } else if a.press.is_some_and(|p| p.2) {
                        // 整个窗口留在鼠标所在显示器的工作区里
                        let r = RECT { left: r0.left + dx, top: r0.top + dy, right: r0.right + dx, bottom: r0.bottom + dy };
                        let (x, y) = fit(&r, work_area(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST)));
                        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    return LRESULT(0);
                }
                if !a.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    a.tracking = true;
                }
                let h = a.hit_at(lparam_pt(lp));
                if h != a.hover {
                    a.hover = h;
                    a.invalidate();
                }
            }
            WM_MOUSELEAVE => {
                a.tracking = false;
                if a.hover.take().is_some() {
                    a.invalidate();
                }
            }
            // 磁贴点击不抢焦点，菜单收不到失活消息：菜单开着时，点磁贴只关菜单，不触发磁贴上的操作（和系统菜单一样）
            WM_LBUTTONDOWN if a.menu.is_some() => menu::close(a),
            WM_LBUTTONDOWN => {
                let mut r = RECT::default();
                let _ = GetWindowRect(hwnd, &mut r);
                a.press = Some((screen_pt(hwnd, lp), r, false));
                SetCapture(hwnd);
            }
            // 先取走按下状态再放开鼠标：ReleaseCapture 发来的 WM_CAPTURECHANGED 就不会再处理一遍
            WM_LBUTTONUP => {
                let pr = a.press.take();
                let _ = ReleaseCapture();
                a.release(pr, Some(lparam_pt(lp)));
            }
            WM_CAPTURECHANGED if a.press.is_some() && lp.0 != hwnd.0 as isize => {
                let pr = a.press.take();
                a.release(pr, None);
            }
            WM_RBUTTONUP => {
                let row = match a.hit_at(lparam_pt(lp)) {
                    Some(Hit::Row(id) | Hit::Login(id)) => Some(id),
                    _ => None,
                };
                a.menu(row);
            }
            WM_KEYDOWN if wp.0 == 'V' as usize && GetKeyState(VK_CONTROL.0 as i32) < 0 => a.import_clip(),
            WM_DROPFILES => {
                let h = HDROP(wp.0 as _);
                let files = sys::drop_files(h);
                DragFinish(h);
                a.start_scan(Src::Files(files));
            }
            WM_MOUSEWHEEL => {
                let max = (a.list.2 - a.list.1).max(0.0);
                a.scroll = (a.scroll - (wp.0 >> 16) as i16 as f32 / 120.0 * view::ROW).clamp(0.0, max);
                a.invalidate();
            }
            // 改名框（以磁贴为所有者的弹出式 Edit）的配色
            WM_CTLCOLOREDIT => {
                let dc = HDC(wp.0 as _);
                let th = Theme::panel(a.theme.dark);
                SetTextColor(dc, colorref(th.p.text));
                SetBkColor(dc, colorref(th.tint));
                return LRESULT(a.edit_brush.0 as isize);
            }
            WM_COMMAND if wp.0 == 1 => a.import_dialog(),
            WM_APP_RENAME => a.start_rename(wp.0 as u32, match lp.0 {
                1 => Field::User,
                2 => Field::Pass,
                _ => Field::Label,
            }),
            WM_APP_LOGIN => {
                let r = *Box::from_raw(lp.0 as *mut std::result::Result<String, String>);
                a.logging_in = false;
                match r {
                    Ok(m) => a.set_note(m, false),
                    Err(m) => a.set_note(m, true),
                }
            }
            WM_APP_SCAN => {
                while let Ok(s) = a.rx.try_recv() {
                    a.on_scan(s);
                }
            }
            WM_SETTINGCHANGE => a.load_theme(),
            // 分辨率或显示器变了，跑到屏幕外就拉回来
            WM_DISPLAYCHANGE => {
                keep_on_screen(hwnd);
                a.save_pos();
            }
            WM_TIMER => match wp.0 {
                T_TICK => a.tick(),
                T_DRAG => a.drag_scroll(),
                T_COPIED => {
                    let _ = KillTimer(Some(hwnd), T_COPIED);
                    a.copied = None;
                    a.invalidate();
                }
                _ => {}
            },
            WM_APP_TRAY => match (lp.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => a.set_hidden(!a.cfg.hidden),
                WM_RBUTTONUP => a.menu(None),
                _ => {}
            },
            WM_DESTROY => {
                a.finish_rename(true);
                menu::close(a);
                a.tray(NIM_DELETE);
                PostQuitMessage(0);
            }
            m if m == a.taskbar_created => a.tray(NIM_ADD),
            _ => return DefWindowProcW(hwnd, msg, wp, lp),
        }
        LRESULT(0)
    }
}

fn main() {
    // 没有控制台，崩溃信息写到 %TEMP%\authtile-crash.txt
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(std::env::temp_dir().join("authtile-crash.txt"), format!("{info}\n{}", std::backtrace::Backtrace::force_capture()));
    }));
    let args: Vec<String> = std::env::args().collect();
    let data = arg(&args, "--data").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("AuthTile"));
    unsafe {
        let mutex = wide(&format!("AuthTile-{}", data.to_string_lossy().replace('\\', "/").to_lowercase()));
        let _guard = CreateMutexW(None, true, PCWSTR(mutex.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let mut cfg = Config::parse(&std::fs::read_to_string(data.join("config.txt")).unwrap_or_default());
        if let Some(l) = arg(&args, "--lang") {
            cfg.lang = Lang::parse(&l);
        }
        config::set_lang(cfg.lang);
        // 文件不存在 = 还没导入过；存在但解不开 = 换了电脑或用户，锁住不写
        let (vault, locked) = match std::fs::read(data.join("vault.dat")) {
            Err(_) => (Vault::default(), false),
            Ok(b) => match sys::protect(&b, false).and_then(|p| String::from_utf8(p).ok()) {
                Some(t) => (Vault::parse(&t), false),
                None => (Vault::default(), true),
            },
        };

        let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("D2D 初始化失败");
        let gfx = gfx::Gfx::new(&d2d).expect("D3D 初始化失败");
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DirectWrite 初始化失败");
        let fonts = Fonts::new(&dw, GetDpiForSystem());

        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        type Proc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
        let classes: [(Proc, PCWSTR); 2] = [(wnd_proc, CLASS), (menu::menu_proc, menu::MENU_CLASS)];
        for (proc_, class) in classes {
            let wc = WNDCLASSW { lpfnWndProc: Some(proc_), lpszClassName: class, hCursor: cursor, ..Default::default() };
            RegisterClassW(&wc);
        }
        let topmost = cfg.topmost || args.iter().any(|a| a == "--top");
        cfg.topmost = topmost;
        // 默认放在主屏工作区右上角
        let s = GetDpiForSystem() as f32 / 96.0;
        let mut wa = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        if cfg.pos[2] == 0 {
            let w = (DEFAULT_W * s) as i32;
            cfg.pos = [wa.right - w - (24.0 * s) as i32, wa.top + (24.0 * s) as i32, w];
        }
        // 工具窗口：不出现在任务栏和 Alt+Tab 里；接受拖进来的图片文件
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_ACCEPTFILES | if topmost { WS_EX_TOPMOST } else { WINDOW_EX_STYLE(0) },
            CLASS,
            w!("Authenticator"),
            WS_POPUP | WS_THICKFRAME,
            cfg.pos[0],
            cfg.pos[1],
            cfg.pos[2],
            (200.0 * s) as i32,
            None,
            None,
            None,
            None,
        )
        .expect("创建窗口失败");

        let (tx, rx) = channel::<Scan>();
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let mut a = Box::new(App {
            data,
            cfg,
            hwnd,
            d2d,
            gfx,
            dw,
            fonts,
            theme: Theme::new(true, 1.0),
            force_dark: if args.iter().any(|a| a == "--light") { Some(false) } else if args.iter().any(|a| a == "--dark") { Some(true) } else { None },
            surf: None,
            sys_dark: true,
            menu: None,
            menu_for: None,
            vault,
            locked,
            note: None,
            copied: None,
            editing: None,
            edit: None,
            edit_brush: HBRUSH::default(),
            hits: Vec::new(),
            hover: None,
            scroll: 0.0,
            list: (0.0, 0.0, 0.0),
            expanded: false,
            drag: None,
            tracking: false,
            press: None,
            logging_in: false,
            // 资源里的应用图标（编号 1），还没打包图标时用系统默认图标
            tray_icon: LoadIconW(Some(hinst.into()), PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default(),
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            rx,
            tx,
        });
        APP = &mut *a;
        a.load_theme();
        a.tray(NIM_ADD);
        if a.locked {
            // 一直显示，不自动消失
            a.note = Some((tr("验证码数据无法解密：只能在原来的电脑和 Windows 用户下打开", "驗證碼資料無法解密：只能在原本的電腦和 Windows 使用者下開啟", "Can't decrypt the codes: open them on the original PC and Windows user").into(), true, u64::MAX));
        }
        a.tick();
        if !a.cfg.hidden {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

//! 右键菜单：自己画的面板，替代系统菜单（系统菜单不跟深色主题，也放不下滑块）。
//! 透明度滑块可以直接拖；开关、分段选项点了面板不关，方便连着调；动作项点了就关。点到面板外面、按 Esc 关闭。
//! 面板底色不透明，不受磁贴透明度影响。键盘：上下选项、左右调滑块和分段、回车或空格确认。

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;
use windows_numerics::Vector2;

use crate::config::{Lang, lang, tr};
use crate::gfx::Surface;
use crate::theme::*;
use crate::view::{Align, P, inside, rect};
use crate::{App, app, sys};

pub const MENU_CLASS: PCWSTR = w!("AuthTileMenu");
const W: f32 = 272.0;
const ROW: f32 = 32.0;
const EDGE: f32 = 8.0;
const X: f32 = 16.0;
pub const OPACITY_MIN: u32 = 20;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Id {
    Opacity,
    Top,
    Auto,
    Lang(Lang),
    ImportFile,
    ImportClip,
    Hide,
    Exit,
    // 以下是某一条验证码的菜单
    Copy,
    Rename,
    Up,
    Down,
    /// 隐藏或取消隐藏这一条
    HideRow,
    /// 填登录账号、密码
    SetLogin,
    /// 开关这一条的登录按钮
    LoginBtn,
    Delete,
}

enum Row {
    Slider,
    Switch(Id, &'static str, bool),
    Seg(&'static str, Vec<(Id, &'static str, bool)>),
    Act(Id, &'static str),
    Sep,
}

pub struct Menu {
    pub hwnd: HWND,
    surf: Option<Surface>,
    hits: Vec<(D2D_RECT_F, Id)>,
    hover: Option<Id>,
    /// 键盘焦点（hits 里的序号），None 表示没用过键盘
    focus: Option<usize>,
    sliding: bool,
    tracking: bool,
    /// 按下时所在的项，松开时还在同一项上才算点击
    pressed: Option<Id>,
    /// 删除点过一次，再点才真删
    armed: bool,
}

/// 右键在某条验证码上（a.menu_for）弹条目菜单，其他地方弹设置菜单
fn rows(a: &App, armed: bool) -> Vec<Row> {
    if let Some(id) = a.menu_for {
        let e = a.vault.get(id);
        let mut rows = vec![
            Row::Act(Id::Copy, tr("复制验证码", "複製驗證碼", "Copy code")),
            Row::Act(Id::Rename, tr("重命名", "重新命名", "Rename")),
            Row::Act(Id::Up, tr("上移", "上移", "Move up")),
            Row::Act(Id::Down, tr("下移", "下移", "Move down")),
            if e.is_some_and(|e| e.hidden) { Row::Act(Id::HideRow, tr("取消隐藏", "取消隱藏", "Unhide")) } else { Row::Act(Id::HideRow, tr("隐藏", "隱藏", "Hide")) },
            Row::Sep,
            Row::Act(Id::SetLogin, tr("设置游戏登录账号…", "設定遊戲登入帳號…", "Set game login…")),
        ];
        // 账号密码设全了才能开登录按钮
        if let Some(e) = e.filter(|e| e.can_login()) {
            rows.push(Row::Switch(Id::LoginBtn, tr("登录按钮", "登入按鈕", "Login button"), e.login));
        }
        rows.extend([Row::Sep, Row::Act(Id::Delete, if armed { tr("再点一次确认删除", "再按一次確認刪除", "Click again to delete") } else { tr("删除", "刪除", "Delete") })]);
        return rows;
    }
    vec![
        Row::Slider,
        Row::Sep,
        Row::Switch(Id::Top, tr("置顶显示", "保持在最上層", "Always on top"), a.cfg.topmost),
        Row::Switch(Id::Auto, tr("开机自启", "開機自動啟動", "Start with Windows"), sys::autostart()),
        // 语言入口和选项都带英文，切到看不懂的语言后也能找回来
        Row::Seg(tr("语言/Language", "語言/Language", "Language"), vec![(Id::Lang(Lang::Zh), "简体中文", lang() == Lang::Zh), (Id::Lang(Lang::Tw), "繁體中文", lang() == Lang::Tw), (Id::Lang(Lang::En), "English", lang() == Lang::En)]),
        Row::Sep,
        Row::Act(Id::ImportFile, tr("导入二维码图片…", "匯入 QR 碼圖片…", "Import QR images…")),
        Row::Act(Id::ImportClip, tr("从剪贴板导入", "從剪貼簿匯入", "Import from clipboard")),
        if a.cfg.hidden { Row::Act(Id::Hide, tr("显示磁贴", "顯示小工具", "Show tile")) } else { Row::Act(Id::Hide, tr("隐藏磁贴", "隱藏小工具", "Hide tile")) },
        Row::Sep,
        Row::Act(Id::Exit, tr("退出", "結束", "Exit")),
    ]
}

fn row_h(r: &Row) -> f32 {
    match r {
        Row::Slider => ROW + 24.0,
        Row::Seg(..) => ROW + 3.0 * U + 2.0 * U + 8.0,
        Row::Sep => 2.0 * U + HAIR,
        _ => ROW,
    }
}

fn height(a: &App) -> f32 {
    2.0 * EDGE + rows(a, false).iter().map(row_h).sum::<f32>()
}

/// 在屏幕坐标 pt 处弹出；靠近屏幕右边或下边时往左、往上翻（托盘在右下角）
pub fn show(a: &mut App, pt: POINT) {
    close(a);
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96, 96);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let s = dx as f32 / 96.0;
        let (w, h) = ((W * s) as i32, (height(a) * s) as i32);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let x = if pt.x + w > wa.right { pt.x - w } else { pt.x };
        let y = if pt.y + h > wa.bottom { pt.y - h } else { pt.y };
        let (x, y) = (x.clamp(wa.left, (wa.right - w).max(wa.left)), y.clamp(wa.top, (wa.bottom - h).max(wa.top)));
        let Ok(hwnd) = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP, MENU_CLASS, w!("AuthTile menu"), WS_POPUP, x, y, w, h, None, None, None, None) else { return };
        let dark = BOOL::from(a.sys_dark);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
        let corner = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as _, 4);
        a.menu = Some(Box::new(Menu { hwnd, surf: None, hits: Vec::new(), hover: None, focus: None, sliding: false, tracking: false, pressed: None, armed: false }));
        let _ = ShowWindow(hwnd, SW_SHOW);
        // 托盘或磁贴刚收到点击，有权把前台给菜单；菜单要在前台才能在点到外面时收到失活消息
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn close(a: &mut App) {
    if let Some(m) = a.menu.take() {
        unsafe {
            let _ = DestroyWindow(m.hwnd);
        }
        a.menu_closed();
    }
}

fn draw(a: &App, m: &mut Menu) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(m.hwnd, &mut rc);
        if m.surf.is_none() {
            m.surf = a.gfx.surface(m.hwnd, rc.right as u32, rc.bottom as u32).ok();
        }
        let Some(surf) = m.surf.clone() else { return };
        let dpi = GetDpiForWindow(m.hwnd) as f32;
        if a.gfx.bind(&surf, dpi).is_err() {
            return;
        }
        let th = Theme::panel(a.sys_dark);
        let c = &th.p;
        let f = &a.fonts;
        let p = P { dc: &a.gfx.dc, br: &a.gfx.brush, dw: &a.dw, d2d: &a.d2d, shadow: false };
        let w = rc.right as f32 * 96.0 / dpi;
        let h = rc.bottom as f32 * 96.0 / dpi;
        let mut hits = Vec::new();
        let focused = m.focus.and_then(|i| m.hits.get(i)).map(|x| x.1);
        let hot = |id: Id| m.hover == Some(id) || focused == Some(id);
        a.gfx.dc.BeginDraw();
        a.gfx.dc.Clear(Some(&th.tint));
        let mut y = EDGE;
        for row in rows(a, m.armed) {
            let rh = row_h(&row);
            match row {
                Row::Slider => {
                    let v = a.cfg.opacity;
                    p.text(tr("磁贴透明度", "小工具透明度", "Tile opacity"), rect(X, y, w - X, y + ROW), &f.b13, c.text, Align::L);
                    p.text(&format!("{v}%"), rect(X, y, w - X, y + ROW), &f.b13b, c.text, Align::R);
                    // 滑块：灰色底线，左边已选的一段是强调色，方形滑钮
                    let (l, r, cy) = (X + 4.0, w - X - 4.0, y + ROW + 2.0 * U);
                    let x = l + (r - l) * (v - OPACITY_MIN) as f32 / (100 - OPACITY_MIN) as f32;
                    p.fill(rect(l, cy - 1.0, r, cy + 1.0), c.border);
                    p.fill(rect(l, cy - 1.0, x, cy + 1.0), c.accent);
                    let tw = if m.sliding || hot(Id::Opacity) { 12.0 } else { 8.0 };
                    p.fill(rect(x - tw / 2.0, cy - 8.0, x + tw / 2.0, cy + 8.0), c.text);
                    if focused == Some(Id::Opacity) {
                        p.outline(rect(0.0, y + ROW, w, y + rh), RULE, c.text);
                    }
                    hits.push((rect(0.0, y + ROW, w, y + rh), Id::Opacity));
                }
                Row::Switch(id, label, on) => {
                    let r = rect(0.0, y, w, y + rh);
                    row_bg(&p, &th, r, hot(id), false);
                    p.text(label, rect(X + 24.0, y, w - X, y + rh), &f.b13, c.text, Align::L);
                    // 复选框：14 px 方框，选中时填强调色、画对勾
                    let b = rect(X, y + rh / 2.0 - 7.0, X + 14.0, y + rh / 2.0 + 7.0);
                    if on {
                        p.fill(b, c.accent);
                        p.color(c.on_accent);
                        a.gfx.dc.DrawLine(Vector2 { X: b.left + 3.0, Y: b.top + 7.5 }, Vector2 { X: b.left + 6.0, Y: b.top + 10.5 }, &a.gfx.brush, 2.0, None);
                        a.gfx.dc.DrawLine(Vector2 { X: b.left + 6.0, Y: b.top + 10.5 }, Vector2 { X: b.left + 11.5, Y: b.top + 3.5 }, &a.gfx.brush, 2.0, None);
                    } else {
                        p.outline(b, HAIR + 0.5, c.text);
                    }
                    if focused == Some(id) {
                        p.outline(r, RULE, c.text);
                    }
                    hits.push((r, id));
                }
                Row::Seg(label, opts) => {
                    p.text(label, rect(X, y, w - X, y + ROW), &f.l12, c.muted, Align::L);
                    // 分段选项：并排的方格，选中的反色（text 底、bg 字），其余 muted 字；格子之间一条细线
                    let bar = rect(X, y + ROW, w - X, y + rh - 2.0 * U);
                    let sw = (bar.right - bar.left) / opts.len() as f32;
                    p.outline(bar, HAIR, c.border);
                    for (k, (id, text, on)) in opts.into_iter().enumerate() {
                        let b = rect(bar.left + k as f32 * sw, bar.top, bar.left + (k + 1) as f32 * sw, bar.bottom);
                        if on {
                            p.fill(b, c.text);
                        } else if hot(id) {
                            p.fill(b, c.surface);
                        }
                        if k > 0 && !on {
                            p.fill(rect(b.left, b.top, b.left + HAIR, b.bottom), c.border);
                        }
                        if focused == Some(id) {
                            p.outline(b, RULE, if on { c.bg } else { c.text });
                        }
                        p.text(text, b, if on { &f.b13b } else { &f.b13 }, if on { c.bg } else { c.muted }, Align::C);
                        hits.push((b, id));
                    }
                }
                Row::Act(id, label) => {
                    let r = rect(0.0, y, w, y + rh);
                    let danger = id == Id::Exit && hot(id) || id == Id::Delete && (hot(id) || m.armed);
                    row_bg(&p, &th, r, hot(id), danger);
                    p.text(label, rect(X, y, w - X, y + rh), &f.b13, if danger { c.red } else { c.text }, Align::L);
                    if focused == Some(id) {
                        p.outline(r, RULE, c.text);
                    }
                    hits.push((r, id));
                }
                Row::Sep => p.fill(rect(0.0, y + rh / 2.0, w, y + rh / 2.0 + HAIR), c.border),
            }
            y += rh;
        }
        // 弹出面板：一圈 1 px 的 text 色边框，没有阴影
        p.outline(rect(0.0, 0.0, w, h), HAIR, c.text);
        let _ = a.gfx.dc.EndDraw(None, None);
        let _ = a.gfx.present(&surf);
        m.hits = hits;
    }
}

/// 菜单行的底：悬停时 surface，左边的结构线变强调色（危险项变红）
fn row_bg(p: &P, th: &Theme, r: D2D_RECT_F, hot: bool, danger: bool) {
    if hot {
        p.fill(r, th.p.surface);
        p.fill(rect(r.left, r.top, r.left + BAR, r.bottom), if danger { th.p.red } else { th.p.accent });
    }
}

/// 执行菜单项，返回菜单是否留着。删除要点两次：第一次只把这一项变成“再点一次确认删除”
fn act(a: &mut App, m: &mut Menu, id: Id) -> bool {
    if id == Id::Delete && !m.armed {
        m.armed = true;
        return true;
    }
    !a.menu_action(id)
}

fn hit(m: &Menu, lp: LPARAM) -> Option<Id> {
    let s = unsafe { GetDpiForWindow(m.hwnd) } as f32 / 96.0;
    let (x, y) = ((lp.0 as i16) as f32 / s, ((lp.0 >> 16) as i16) as f32 / s);
    m.hits.iter().find(|(r, _)| inside(r, x, y)).map(|(_, id)| *id)
}

/// 滑块按鼠标横坐标取值，5% 一档
fn slide(a: &mut App, m: &Menu, lp: LPARAM) {
    let s = unsafe { GetDpiForWindow(m.hwnd) } as f32 / 96.0;
    let x = (lp.0 as i16) as f32 / s;
    let (l, r) = (X + 4.0, W - X - 4.0);
    let t = ((x - l) / (r - l)).clamp(0.0, 1.0);
    let v = OPACITY_MIN + ((t * (100 - OPACITY_MIN) as f32 / 5.0).round() as u32) * 5;
    if v != a.cfg.opacity {
        a.set_opacity(v);
    }
}

pub unsafe extern "system" fn menu_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => return LRESULT(0),
            WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
            WM_ERASEBKGND => return LRESULT(1),
            _ => {}
        }
        let a = app();
        // 菜单先从 App 里拿出来，处理完再放回去：动作可能要改 App 的其他部分，甚至关掉菜单
        let Some(mut m) = a.menu.take_if(|m| m.hwnd == hwnd) else {
            return DefWindowProcW(hwnd, msg, wp, lp);
        };
        let mut keep = true;
        let mut redraw = false;
        match msg {
            WM_PAINT => {
                let _ = ValidateRect(Some(hwnd), None);
                draw(a, &mut m);
            }
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => keep = false,
            WM_MOUSEMOVE => {
                if !m.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    m.tracking = true;
                }
                if m.sliding {
                    slide(a, &m, lp);
                }
                let h = hit(&m, lp);
                redraw = h != m.hover || m.sliding || m.focus.is_some();
                m.hover = h;
                m.focus = None;
            }
            WM_MOUSELEAVE => {
                m.tracking = false;
                redraw = m.hover.take().is_some();
            }
            WM_LBUTTONDOWN => {
                m.pressed = hit(&m, lp);
                if m.pressed == Some(Id::Opacity) {
                    m.sliding = true;
                    SetCapture(hwnd);
                    slide(a, &m, lp);
                }
                redraw = true;
            }
            WM_LBUTTONUP => {
                if std::mem::take(&mut m.sliding) {
                    let _ = ReleaseCapture();
                    a.save_cfg();
                } else if let Some(id) = hit(&m, lp).filter(|id| m.pressed == Some(*id)) {
                    keep = act(a, &mut m, id);
                }
                m.pressed = None;
                redraw = true;
            }
            WM_KEYDOWN => {
                let n = m.hits.len();
                let cur = m.focus.or_else(|| m.hover.and_then(|h| m.hits.iter().position(|x| x.1 == h)));
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_ESCAPE => keep = false,
                    VK_DOWN | VK_TAB if n > 0 => m.focus = Some(cur.map_or(0, |i| (i + 1) % n)),
                    VK_UP if n > 0 => m.focus = Some(cur.map_or(n - 1, |i| (i + n - 1) % n)),
                    k @ (VK_LEFT | VK_RIGHT) => match cur.map(|i| m.hits[i].1) {
                        Some(Id::Opacity) => {
                            let v = if k == VK_LEFT { a.cfg.opacity.saturating_sub(5).max(OPACITY_MIN) } else { (a.cfg.opacity + 5).min(100) };
                            a.set_opacity(v);
                            a.save_cfg();
                            m.focus = cur;
                        }
                        // 分段选项里左右移动就是换选项
                        Some(Id::Lang(_)) => {
                            let i = cur.unwrap_or(0);
                            let j = if k == VK_LEFT { i.saturating_sub(1) } else { (i + 1).min(n - 1) };
                            if std::mem::discriminant(&m.hits[i].1) == std::mem::discriminant(&m.hits[j].1) {
                                let id = m.hits[j].1;
                                keep = act(a, &mut m, id);
                                m.focus = Some(j);
                            }
                        }
                        _ => {}
                    },
                    VK_RETURN | VK_SPACE => {
                        if let Some(i) = cur {
                            let id = m.hits[i].1;
                            keep = act(a, &mut m, id);
                            m.focus = Some(i);
                        }
                    }
                    _ => {}
                }
                redraw = true;
            }
            WM_DPICHANGED => {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                m.surf = None;
            }
            _ => {
                a.menu = Some(m);
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
        }
        if keep {
            // 动作可能改了行数（如“隐藏磁贴”变“显示磁贴”）不会改高度；语言切换会改文字，整块重画
            if redraw {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            if a.menu.is_none() {
                a.menu = Some(m);
            }
        } else {
            let _ = DestroyWindow(m.hwnd);
            a.menu_closed();
        }
        LRESULT(0)
    }
}

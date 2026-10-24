//! 磁贴的排版和绘制。尺寸都是 DIP，绘制时由渲染目标按窗口 DPI 缩放。
//! 自上而下：标题行（名字、导入按钮、置顶按钮）、导入结果提示（几秒后消失）、验证码列表。高度随条目数变，放不下屏幕时列表滚动。
//! 列表末尾是已隐藏验证码的展开按钮，展开后隐藏的一组变淡显示在按钮下面。

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;
use windows_numerics::{Matrix3x2, Vector2};

use crate::App;
use crate::config::tr;

pub const PAD: f32 = 14.0;
pub const MIN_W: f32 = 240.0;
pub const HEAD: f32 = 46.0;
pub const ROW: f32 = 54.0;
const EMPTY: f32 = 112.0;
const BOTTOM: f32 = 8.0;
/// 展开按钮那一行
const MORE: f32 = 36.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    /// 一条验证码（条目编号），点了复制
    Row(u32),
    Import,
    Pin,
    /// 展开、收起已隐藏的验证码
    More,
}

/// 拖动排序中：哪一条、按下时鼠标在这一行里的高度、鼠标现在的纵坐标（客户区 DIP）
pub struct Drag {
    pub id: u32,
    pub grab: f32,
    pub y: f32,
}

/// 列表的排布，位置相对列表顶、未滚动
pub struct Layout {
    /// 每行的条目下标和行顶；拖动中的行放在最后，画在最上面
    pub rows: Vec<(usize, f32)>,
    /// 展开按钮的顶和已隐藏的条数；没有隐藏的就没有按钮
    pub more: Option<(f32, usize)>,
    pub h: f32,
    /// 拖动中的行落在组里第几个
    pub slot: usize,
}

/// 先排显示的一组；有隐藏的就接展开按钮，展开时按钮下面排隐藏的一组
pub fn layout(a: &App) -> Layout {
    let es = &a.vault.entries;
    let n_hidden = es.iter().filter(|e| e.hidden).count();
    let mut lay = Layout { rows: Vec::new(), more: None, h: 0.0, slot: 0 };
    let mut dragged = None;
    for hidden in [false, true] {
        if hidden {
            if n_hidden == 0 {
                break;
            }
            lay.more = Some((lay.h, n_hidden));
            lay.h += MORE;
            if !a.expanded {
                break;
            }
        }
        let top = lay.h;
        let mut g: Vec<usize> = (0..es.len()).filter(|&i| es[i].hidden == hidden).collect();
        // 拖动的行跟着鼠标，限制在本组范围里；行离哪一格近就落在哪，组里其它行给它让位
        if let Some(d) = &a.drag
            && let Some(k) = g.iter().position(|&i| es[i].id == d.id)
        {
            let y = (d.y - a.list.0 + a.scroll - d.grab).clamp(top, top + (g.len() - 1) as f32 * ROW);
            lay.slot = ((y - top) / ROW).round() as usize;
            let i = g.remove(k);
            g.insert(lay.slot, i);
            dragged = Some((i, y));
        }
        for (k, &i) in g.iter().enumerate() {
            if dragged.is_none_or(|(d, _)| d != i) {
                lay.rows.push((i, top + k as f32 * ROW));
            }
        }
        lay.h += g.len() as f32 * ROW;
    }
    lay.rows.extend(dragged);
    lay
}

pub struct Theme {
    pub dark: bool,
    pub tint: D2D1_COLOR_F,
    pub text: D2D1_COLOR_F,
    pub muted: D2D1_COLOR_F,
    pub faint: D2D1_COLOR_F,
    pub track: D2D1_COLOR_F,
    pub hover: D2D1_COLOR_F,
    pub accent: D2D1_COLOR_F,
    pub danger: D2D1_COLOR_F,
    /// 底色太薄时文字压在壁纸上，加阴影（和桌面图标一样）
    pub shadow: bool,
}

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

pub fn with_a(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: c.a * a, ..c }
}

impl Theme {
    /// 磁贴的配色；opacity 是底色的不透明度（0.2～1）。灰阶和 ClaudeTile / Nookery 一致，强调色是本程序的青绿
    pub fn new(dark: bool, opacity: f32) -> Theme {
        let (bg, fg) = if dark { (0x20 as f32 / 255.0, 0xF3 as f32 / 255.0) } else { (0xF3 as f32 / 255.0, 0x1A as f32 / 255.0) };
        let g = |a: f32| rgba(fg, fg, fg, a);
        Theme {
            dark,
            tint: rgba(bg, bg, bg, opacity * if dark { 0.80 } else { 0.88 }),
            shadow: opacity < 0.5,
            text: g(1.0),
            muted: g(if dark { 0.66 } else { 0.64 }),
            faint: g(if dark { 0.09 } else { 0.08 }),
            track: g(if dark { 0.14 } else { 0.12 }),
            hover: g(0.07),
            accent: if dark { rgba(0.243, 0.788, 0.725, 1.0) } else { rgba(0.047, 0.533, 0.486, 1.0) },
            danger: if dark { rgba(0.957, 0.392, 0.341, 1.0) } else { rgba(0.769, 0.169, 0.110, 1.0) },
        }
    }

    /// 右键菜单用的不透明面板：不跟磁贴的透明度走
    pub fn panel(dark: bool) -> Theme {
        let bg = if dark { 0x2C as f32 / 255.0 } else { 0xF9 as f32 / 255.0 };
        Theme { tint: rgba(bg, bg, bg, 1.0), ..Theme::new(dark, 1.0) }
    }
}

pub struct Fonts {
    pub f11: IDWriteTextFormat,
    pub f12: IDWriteTextFormat,
    pub f12b: IDWriteTextFormat,
    pub f14b: IDWriteTextFormat,
    /// 验证码数字
    pub code: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    pub icon_big: IDWriteTextFormat,
    /// 12 号、可换行，提示条用
    pub f12w: IDWriteTextFormat,
    /// 编辑框用的 GDI 字体（12 号）
    pub edit: windows::Win32::Graphics::Gdi::HFONT,
}

impl Fonts {
    pub fn new(dw: &IDWriteFactory, dpi: u32) -> Fonts {
        let mk = |fam: PCWSTR, size: f32, weight: DWRITE_FONT_WEIGHT| unsafe {
            let f = dw.CreateTextFormat(fam, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, crate::config::locale()).expect("创建字体失败");
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&f) {
                let _ = f.SetTrimming(&trim, &sign);
            }
            f
        };
        let text = w!("Segoe UI Variable Text");
        // Win11 的图标字体是 Segoe Fluent Icons，Win10 只有 Segoe MDL2 Assets，两者用到的码位相同
        let icon_fam = unsafe {
            let mut coll = None;
            let _ = dw.GetSystemFontCollection(&mut coll, false);
            let (mut idx, mut found) = (0u32, BOOL(0));
            if let Some(c) = coll {
                let _ = c.FindFamilyName(w!("Segoe Fluent Icons"), &mut idx, &mut found);
            }
            if found.as_bool() { w!("Segoe Fluent Icons") } else { w!("Segoe MDL2 Assets") }
        };
        // 12 号、可换行、顶对齐，提示条用
        let f12w = mk(text, 12.0, DWRITE_FONT_WEIGHT_NORMAL);
        unsafe {
            let _ = f12w.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = f12w.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
        }
        let edit = unsafe {
            use windows::Win32::Graphics::Gdi::*;
            let h = -(12.0 * dpi as f32 / 72.0) as i32;
            CreateFontW(h, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, w!("Segoe UI Variable Text"))
        };
        Fonts {
            f11: mk(text, 11.0, DWRITE_FONT_WEIGHT_NORMAL),
            f12: mk(text, 12.0, DWRITE_FONT_WEIGHT_NORMAL),
            f12b: mk(text, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD),
            f14b: mk(text, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD),
            code: mk(w!("Segoe UI Variable Display"), 22.0, DWRITE_FONT_WEIGHT_SEMI_BOLD),
            icon: mk(icon_fam, 14.0, DWRITE_FONT_WEIGHT_NORMAL),
            icon_big: mk(icon_fam, 28.0, DWRITE_FONT_WEIGHT_NORMAL),
            f12w,
            edit,
        }
    }
}

pub fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: l, top: t, right: r, bottom: b }
}

pub fn inside(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// 验证码分两半显示：774 800、1234 5678
pub fn split_code(c: &str) -> String {
    let h = c.len() / 2;
    format!("{} {}", &c[..h], &c[h..])
}

pub struct P<'a> {
    pub dc: &'a ID2D1DeviceContext,
    pub br: &'a ID2D1SolidColorBrush,
    pub dw: &'a IDWriteFactory,
    pub d2d: &'a ID2D1Factory1,
    /// 文字加阴影
    pub shadow: bool,
}

#[derive(Clone, Copy)]
pub enum Align {
    L,
    C,
    R,
}

impl P<'_> {
    pub fn color(&self, c: D2D1_COLOR_F) {
        unsafe { self.br.SetColor(&c) };
    }

    pub fn fill(&self, r: D2D_RECT_F, radius: f32, c: D2D1_COLOR_F) {
        self.color(c);
        unsafe { self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, self.br) };
    }

    pub fn dot(&self, x: f32, y: f32, r: f32, c: D2D1_COLOR_F) {
        self.color(c);
        unsafe { self.dc.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: r, radiusY: r }, self.br) };
    }

    /// 倒计时圆环：灰色底圈，上面从 12 点顺时针画剩余的比例
    pub fn ring(&self, cx: f32, cy: f32, r: f32, frac: f32, track: D2D1_COLOR_F, c: D2D1_COLOR_F) {
        unsafe {
            self.color(track);
            self.dc.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.br, 2.5, None);
            let frac = frac.clamp(0.0, 1.0);
            if frac <= 0.0 {
                return;
            }
            if frac >= 0.999 {
                self.color(c);
                self.dc.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.br, 2.5, None);
                return;
            }
            let Ok(path) = self.d2d.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            let a = frac * std::f32::consts::TAU;
            sink.BeginFigure(Vector2 { X: cx, Y: cy - r }, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2 { X: cx + r * a.sin(), Y: cy - r * a.cos() },
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.color(c);
            self.dc.DrawGeometry(&path, self.br, 2.5, None);
        }
    }

    /// 文字在宽度 w 内排版后的高度（折行的字体用）
    pub fn measure(&self, s: &str, w: f32, f: &IDWriteTextFormat) -> f32 {
        let s: Vec<u16> = s.encode_utf16().collect();
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe {
            if let Ok(l) = self.dw.CreateTextLayout(&s, f, w.max(1.0), 1000.0) {
                let _ = l.GetMetrics(&mut m);
            }
        }
        m.height
    }

    /// 在 r 里画一行字，返回文字实际宽度
    pub fn text(&self, s: &str, r: D2D_RECT_F, f: &IDWriteTextFormat, c: D2D1_COLOR_F, al: Align) -> f32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let Ok(l) = self.dw.CreateTextLayout(&w, f, (r.right - r.left).max(1.0), (r.bottom - r.top).max(1.0)) else { return 0.0 };
            let _ = l.SetTextAlignment(match al {
                Align::L => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::C => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::R => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            if self.shadow {
                self.color(rgba(0.0, 0.0, 0.0, 0.55 * c.a));
                self.dc.DrawTextLayout(Vector2 { X: r.left + 1.0, Y: r.top + 1.0 }, &l, self.br, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
            }
            self.color(c);
            // 允许彩色字形：名字里可以有 emoji
            self.dc.DrawTextLayout(Vector2 { X: r.left, Y: r.top }, &l, self.br, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            m.width
        }
    }
}

/// 一帧的排版结果
pub struct Frame {
    /// 内容总高度（不受屏幕限制），窗口按它定高
    pub h: f32,
    /// 列表区域的 (顶, 可见高度, 总高)，滚轮、拖动排序和改名框定位用
    pub list: (f32, f32, f32),
    pub hits: Vec<(D2D_RECT_F, Hit)>,
}

/// 行顶为 y 的一行在列表里的矩形
pub fn row_rect(y: f32, w: f32) -> D2D_RECT_F {
    rect(PAD - 6.0, y, w - PAD + 6.0, y + ROW - 2.0)
}

/// 改名框放在行顶为 y 的那一行的名字处（客户区 DIP）
pub fn label_rect(a: &App, y: f32, w: f32) -> D2D_RECT_F {
    let top = a.list.0 + y - a.scroll;
    rect(PAD, top + 7.0, w - PAD - 120.0, top + 29.0)
}

/// 画整块磁贴；max_h 是窗口能有的最大高度（工作区高）
pub fn draw(a: &App, w: f32, h: f32, max_h: f32, now: u64) -> Frame {
    let p = P { dc: &a.gfx.dc, br: &a.gfx.brush, dw: &a.dw, d2d: &a.d2d, shadow: a.theme.shadow };
    let (th, f) = (&a.theme, &a.fonts);
    let mut hits = Vec::new();
    let (l, r) = (PAD, w - PAD);
    unsafe {
        p.dc.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
        p.fill(rect(0.0, 0.0, w, h), 0.0, th.tint);
    }

    // 标题行：名字，右边是导入、置顶两个按钮
    let y = 11.0;
    p.text("Authenticator", rect(l, y, r - 60.0, y + 24.0), &f.f14b, th.text, Align::L);
    // Segoe Fluent Icons：E710 加号，E718 图钉，E840 按下的图钉
    let buttons = [(Hit::Import, "\u{E710}", th.muted), (Hit::Pin, if a.cfg.topmost { "\u{E840}" } else { "\u{E718}" }, if a.cfg.topmost { th.accent } else { th.muted })];
    for (k, (hit, glyph, c)) in buttons.into_iter().enumerate() {
        let b = rect(r - 52.0 + k as f32 * 28.0, y, r - 24.0 + k as f32 * 28.0 + 4.0, y + 24.0);
        if a.hover == Some(hit) {
            p.fill(b, 5.0, th.hover);
        }
        p.text(glyph, b, &f.icon, c, Align::C);
        hits.push((b, hit));
    }
    let mut top = HEAD;

    // 导入结果、出错提示；长的折行
    if let Some((msg, err)) = a.note() {
        let nh = p.measure(msg, r - l - 24.0, &f.f12w).max(16.0) + 10.0;
        let b = rect(l - 6.0, top, r + 6.0, top + nh);
        p.fill(b, 6.0, if err { with_a(th.danger, 0.16) } else { th.faint });
        // E946 信息，E7BA 警告；图标对齐第一行
        p.text(if err { "\u{E7BA}" } else { "\u{E946}" }, rect(l, b.top + 5.0, l + 16.0, b.top + 21.0), &f.icon, if err { th.danger } else { th.accent }, Align::C);
        p.text(msg, rect(l + 24.0, b.top + 5.0, r, b.bottom - 5.0), &f.f12w, th.text, Align::L);
        top += nh + 6.0;
    }

    let entries = &a.vault.entries;
    // 数据解不开时只显示上面的提示，不引导导入
    if a.locked {
        return Frame { h: top + BOTTOM, list: (top, 0.0, 0.0), hits };
    }
    if entries.is_empty() {
        p.text("\u{ED14}", rect(l, top + 6.0, r, top + 46.0), &f.icon_big, th.muted, Align::C);
        p.text(tr("把验证器导出的二维码图片拖到这里", "把驗證器匯出的 QR 碼圖片拖到這裡", "Drop exported QR code images here"), rect(l, top + 52.0, r, top + 72.0), &f.f12, th.text, Align::C);
        p.text(tr("或点 + 选择图片、Ctrl+V 粘贴截图", "或按 + 選擇圖片、Ctrl+V 貼上截圖", "or click + to pick, Ctrl+V to paste"), rect(l, top + 74.0, r, top + 92.0), &f.f11, th.muted, Align::C);
        let total = top + EMPTY;
        return Frame { h: total, list: (top, 0.0, 0.0), hits };
    }

    // 验证码列表：放不下时裁剪并按 a.scroll 滚动
    let lay = layout(a);
    let vis = lay.h.min((max_h - top - BOTTOM).max(ROW));
    // 列表里的矩形换到客户区、裁到可见部分，记为可点区域
    let mut hit = |b: D2D_RECT_F, h: Hit| {
        let (t, bt) = (top + b.top - a.scroll, top + b.bottom - a.scroll);
        if bt > top && t < top + vis {
            hits.push((rect(b.left, t.max(top), b.right, bt.min(top + vis)), h));
        }
    };
    unsafe {
        p.dc.PushAxisAlignedClip(&rect(0.0, top, w, top + vis), D2D1_ANTIALIAS_MODE_ALIASED);
        p.dc.SetTransform(&Matrix3x2::translation(0.0, top - a.scroll));
    }
    let cx = r - 9.0;
    if let Some((y, n)) = lay.more {
        let b = rect(PAD - 6.0, y + 3.0, w - PAD + 6.0, y + MORE - 5.0);
        if a.hover == Some(Hit::More) {
            p.fill(b, 6.0, th.hover);
        }
        let s = if a.expanded { tr("收起", "收合", "Collapse").to_string() } else { tr("显示 {0} 个已隐藏", "顯示 {0} 個已隱藏", "Show {0} hidden").replace("{0}", &n.to_string()) };
        p.text(&s, rect(l, b.top, cx - 18.0, b.bottom), &f.f12, th.muted, Align::L);
        // E70E 向上、E70D 向下的箭头，和倒计时圆环对齐
        p.text(if a.expanded { "\u{E70E}" } else { "\u{E70D}" }, rect(cx - 10.0, b.top, cx + 10.0, b.bottom), &f.icon, th.muted, Align::C);
        hit(b, Hit::More);
    }
    for &(i, ry) in &lay.rows {
        let e = &entries[i];
        let rr = row_rect(ry, w);
        if a.drag.as_ref().is_some_and(|d| d.id == e.id) {
            // 拿起的行盖在别的行上面：先铺不透明的底色
            p.fill(rr, 6.0, D2D1_COLOR_F { a: 1.0, ..th.tint });
            p.fill(rr, 6.0, th.track);
        } else if a.hover == Some(Hit::Row(e.id)) || a.menu_for == Some(e.id) {
            p.fill(rr, 6.0, th.hover);
        }
        // 已隐藏的一组变淡
        let dim = |c: D2D1_COLOR_F| if e.hidden { with_a(c, 0.5) } else { c };
        let left = e.otp.remaining(now);
        // 最后 5 秒变红，和手机上的验证器一样提醒快换码了
        let urgent = left <= 5;
        p.ring(cx, ry + ROW / 2.0 - 1.0, 8.0, left as f32 / e.otp.period as f32, dim(th.track), dim(if urgent { th.danger } else { th.accent }));
        let code_r = rect(l, ry, cx - 18.0, ry + ROW - 2.0);
        let cw = if a.copied == Some(e.id) {
            p.text(tr("已复制", "已複製", "Copied"), code_r, &f.f12b, th.accent, Align::R)
        } else {
            p.text(&split_code(&e.otp.code(now)), code_r, &f.code, dim(if urgent { th.danger } else { th.text }), Align::R)
        };
        let lr = (cx - 18.0 - cw - 12.0).max(l + 40.0);
        let editing = a.editing == Some(e.id);
        if !editing {
            p.text(&e.label, rect(l, ry + 7.0, lr, ry + 29.0), &f.f12b, dim(th.text), Align::L);
        }
        let sub = if e.label == e.issuer || e.issuer.is_empty() { e.name.clone() } else if e.name.is_empty() { e.issuer.clone() } else { format!("{} · {}", e.issuer, e.name) };
        let sub = if sub == e.label { String::new() } else { sub };
        p.text(&sub, rect(l, ry + 27.0, lr, ry + 45.0), &f.f11, dim(th.muted), Align::L);
        hit(rr, Hit::Row(e.id));
    }
    unsafe {
        p.dc.SetTransform(&Matrix3x2::identity());
        p.dc.PopAxisAlignedClip();
    }
    Frame { h: top + lay.h + BOTTOM, list: (top, vis, lay.h), hits }
}

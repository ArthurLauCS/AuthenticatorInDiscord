//! 磁贴的排版和绘制。尺寸都是 DIP，绘制时由渲染目标按窗口 DPI 缩放；颜色、字号、间距一律取自 theme.rs。
//! 自上而下：标题行（名字、导入按钮、置顶按钮）、导入结果提示（几秒后消失）、验证码列表。高度随条目数变，放不下屏幕时列表滚动。
//! 列表末尾是已隐藏验证码的展开按钮，展开后隐藏的一组变淡显示在按钮下面。
//! 结构靠线：顶上一条粗线，行与行之间一条细线，行左边一条结构线（悬停时变强调色）；没有卡片、圆角和阴影。

use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::*;
use windows_numerics::{Matrix3x2, Vector2};

use crate::config::tr;
use crate::fontload::Bundled;
use crate::theme::*;
use crate::{App, Field};

pub const MIN_W: f32 = 240.0;
/// 标题行：顶上的粗线加 38 px 的标题
pub const HEAD: f32 = 40.0;
pub const ROW: f32 = 64.0;
const EMPTY: f32 = 96.0;
const BOTTOM: f32 = 0.0;
/// 展开按钮那一行
const MORE: f32 = 40.0;
/// 标题栏右边每个按钮的宽
const BTN: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    /// 一条验证码（条目编号），点了复制
    Row(u32),
    /// 这一条的登录按钮，点了登录启动器
    Login(u32),
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

pub struct Fonts {
    /// 12：标签、副标题
    pub l12: IDWriteTextFormat,
    pub l12b: IDWriteTextFormat,
    /// 13：正文、菜单；粗体用于账号名
    pub b13: IDWriteTextFormat,
    pub b13b: IDWriteTextFormat,
    /// 16 粗：标题、“已复制”
    pub s16b: IDWriteTextFormat,
    /// 24 粗：验证码（Heros 的数字等宽）
    pub code: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    /// 12 号、可换行，提示条用
    pub wrap: IDWriteTextFormat,
    /// 编辑框用的 GDI 字体（13 粗，和被盖住的账号名一样）
    pub edit: windows::Win32::Graphics::Gdi::HFONT,
}

impl Fonts {
    pub fn new(dw: &IDWriteFactory, dpi: u32) -> Fonts {
        let bundled = Bundled::load(dw);
        // 字体文件缺了就退回 Arial，不报错；汉字落到界面语言对应的系统黑体
        let (family, coll) = match &bundled.latin {
            Some(c) => (FONT_LATIN, Some(c)),
            None => (FONT_LATIN_FALLBACK, None),
        };
        let mk = |fam: &str, coll: Option<&IDWriteFontCollection>, size: f32, weight: DWRITE_FONT_WEIGHT| unsafe {
            let name: Vec<u16> = fam.encode_utf16().chain([0]).collect();
            let f = dw.CreateTextFormat(PCWSTR(name.as_ptr()), coll, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, crate::config::locale()).expect("创建字体失败");
            if let Some(fb) = &bundled.fallback
                && let Ok(f1) = f.cast::<IDWriteTextFormat1>()
            {
                let _ = f1.SetFontFallback(fb);
            }
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            if let Ok(sign) = dw.CreateEllipsisTrimmingSign(&f) {
                let _ = f.SetTrimming(&trim, &sign);
            }
            f
        };
        let (reg, bold) = (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD);
        // Win11 的图标字体是 Segoe Fluent Icons，Win10 只有 Segoe MDL2 Assets，两者用到的码位相同
        let icon_fam = unsafe {
            let mut sys = None;
            let _ = dw.GetSystemFontCollection(&mut sys, false);
            let (mut idx, mut found) = (0u32, BOOL(0));
            let name: Vec<u16> = FONT_ICON.encode_utf16().chain([0]).collect();
            if let Some(c) = sys {
                let _ = c.FindFamilyName(PCWSTR(name.as_ptr()), &mut idx, &mut found);
            }
            if found.as_bool() { FONT_ICON } else { FONT_ICON_OLD }
        };
        // 12 号、可换行、顶对齐，提示条用
        let wrap = mk(family, coll, S_LABEL, reg);
        unsafe {
            let _ = wrap.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = wrap.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            // 折行的文字不要省略号（整段都显示，高度由 measure 算出）
            let _ = wrap.SetTrimming(&DWRITE_TRIMMING::default(), None);
        }
        let edit = unsafe {
            use windows::Win32::Graphics::Gdi::*;
            let h = -(S_BODY * dpi as f32 / 96.0).round() as i32;
            let face: Vec<u16> = (if bundled.gdi { FONT_LATIN_GDI } else { FONT_LATIN_FALLBACK }).encode_utf16().chain([0]).collect();
            CreateFontW(h, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, DEFAULT_PITCH.0 as u32, PCWSTR(face.as_ptr()))
        };
        // 繁体界面的汉字用正黑体、简体用雅黑：fontload 的后备链按当前语言建，语言切换时 Fonts 重建
        Fonts {
            l12: mk(family, coll, S_LABEL, reg),
            l12b: mk(family, coll, S_LABEL, bold),
            b13: mk(family, coll, S_BODY, reg),
            b13b: mk(family, coll, S_BODY, bold),
            s16b: mk(family, coll, S_SECTION, bold),
            code: mk(family, coll, S_DISPLAY, bold),
            icon: mk(icon_fam, None, S_ICON, reg),
            wrap,
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

    /// 方角填充（线、底色块都用它）
    pub fn fill(&self, r: D2D_RECT_F, c: D2D1_COLOR_F) {
        self.color(c);
        unsafe { self.dc.FillRectangle(&r, self.br) };
    }

    /// 方角描边，线画在矩形里面
    pub fn outline(&self, r: D2D_RECT_F, w: f32, c: D2D1_COLOR_F) {
        self.fill(rect(r.left, r.top, r.right, r.top + w), c);
        self.fill(rect(r.left, r.bottom - w, r.right, r.bottom), c);
        self.fill(rect(r.left, r.top + w, r.left + w, r.bottom - w), c);
        self.fill(rect(r.right - w, r.top + w, r.right, r.bottom - w), c);
    }

    /// 倒计时圆环：底圈，上面从 12 点顺时针画剩余的比例
    pub fn ring(&self, cx: f32, cy: f32, r: f32, frac: f32, track: D2D1_COLOR_F, c: D2D1_COLOR_F) {
        unsafe {
            self.color(track);
            self.dc.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.br, RING_W, None);
            let frac = frac.clamp(0.0, 1.0);
            if frac <= 0.0 {
                return;
            }
            if frac >= 0.999 {
                self.color(c);
                self.dc.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.br, RING_W, None);
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
            self.dc.DrawGeometry(&path, self.br, RING_W, None);
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

/// 行顶为 y 的一行在列表里的矩形（不含底下的细线）
pub fn row_rect(y: f32, w: f32) -> D2D_RECT_F {
    rect(0.0, y, w, y + ROW - HAIR)
}

/// 改名框放在行顶为 y 的那一行的名字处（客户区 DIP）
pub fn label_rect(a: &App, y: f32, w: f32) -> D2D_RECT_F {
    let top = a.list.0 + y - a.scroll;
    rect(PAD, top + 3.0 * U, w - PAD - 2.0 * RING_R - 5.0 * U, top + 3.0 * U + 20.0)
}

/// 画整块磁贴；max_h 是窗口能有的最大高度（工作区高）
pub fn draw(a: &App, w: f32, h: f32, max_h: f32, now: u64) -> Frame {
    let p = P { dc: &a.gfx.dc, br: &a.gfx.brush, dw: &a.dw, d2d: &a.d2d, shadow: a.theme.shadow };
    let (th, f) = (&a.theme, &a.fonts);
    let c = &th.p;
    let mut hits = Vec::new();
    let (l, r) = (PAD, w - PAD);
    unsafe {
        p.dc.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
    }
    p.fill(rect(0.0, 0.0, w, h), th.tint);

    // 标题行：顶上一条粗线，名字，右边是导入、置顶两个按钮
    p.fill(rect(0.0, 0.0, w, RULE), c.text);
    p.text("Authenticator", rect(l, RULE, r - 2.0 * BTN, HEAD - HAIR), &f.s16b, c.text, Align::L);
    // Segoe Fluent Icons：E710 加号，E718 图钉，E840 按下的图钉
    let buttons = [(Hit::Import, "\u{E710}", c.muted), (Hit::Pin, if a.cfg.topmost { "\u{E840}" } else { "\u{E718}" }, if a.cfg.topmost { c.accent } else { c.muted })];
    for (k, (hit, glyph, col)) in buttons.into_iter().enumerate() {
        let b = rect(r - 2.0 * BTN + k as f32 * BTN, RULE, r - BTN + k as f32 * BTN, HEAD - HAIR);
        let hot = a.hover == Some(hit);
        if hot {
            p.fill(b, th.surface());
        }
        p.text(glyph, b, &f.icon, if hot && hit == Hit::Import { c.accent } else if hot { c.text } else { col }, Align::C);
        hits.push((b, hit));
    }
    p.fill(rect(0.0, HEAD - HAIR, w, HEAD), c.border);
    let mut top = HEAD;

    // 导入结果、出错提示：左边一条结构线（出错是红色），长的折行
    if let Some((msg, err)) = a.note() {
        let nh = p.measure(msg, r - l, &f.wrap).max(16.0) + 3.0 * U;
        p.fill(rect(0.0, top, w, top + nh), th.surface());
        p.fill(rect(0.0, top, BAR, top + nh), if err { c.red } else { c.accent });
        p.text(msg, rect(l, top + 1.5 * U, r, top + nh - 1.5 * U), &f.wrap, c.text, Align::L);
        p.fill(rect(0.0, top + nh - HAIR, w, top + nh), c.border);
        top += nh;
    }

    let entries = &a.vault.entries;
    // 数据解不开时只显示上面的提示，不引导导入
    if a.locked {
        return Frame { h: top + BOTTOM, list: (top, 0.0, 0.0), hits };
    }
    if entries.is_empty() {
        // 一行说明，下一步是一个文字链接；靠左、贴着内容顶
        p.text(tr("把验证器导出的二维码图片拖到这里", "把驗證器匯出的 QR 碼圖片拖到這裡", "Drop exported QR code images here"), rect(l, top + 4.0 * U, r, top + 9.0 * U), &f.b13, c.text, Align::L);
        let lw = p.text(tr("导入二维码图片 →", "匯入 QR 碼圖片 →", "Import QR images →"), rect(l, top + 10.0 * U, r, top + 15.0 * U), &f.b13b, if a.hover == Some(Hit::Import) { c.text } else { c.accent }, Align::L);
        hits.push((rect(l, top + 10.0 * U, l + lw, top + 15.0 * U), Hit::Import));
        p.text(tr("或按 Ctrl+V 粘贴截图", "或按 Ctrl+V 貼上截圖", "or press Ctrl+V to paste a screenshot"), rect(l, top + 15.0 * U, r, top + 20.0 * U), &f.l12, c.muted, Align::L);
        return Frame { h: top + EMPTY, list: (top, 0.0, 0.0), hits };
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
    let ring_cx = r - RING_R;
    // 验证码右边缘：圆环左边留 12
    let code_end = ring_cx - RING_R - 3.0 * U;
    if let Some((y, n)) = lay.more {
        let b = rect(0.0, y, w, y + MORE);
        let hot = a.hover == Some(Hit::More);
        if hot {
            p.fill(rect(0.0, y + RULE, w, y + MORE - HAIR), th.surface());
        }
        // 展开按钮是下一小节的开头：一条粗线，hover 时左边的结构线变强调色
        p.fill(rect(0.0, y, w, y + RULE), c.text);
        p.fill(rect(0.0, y + RULE, BAR, y + MORE - HAIR), if hot { c.accent } else { c.border });
        let s = if a.expanded { tr("收起已隐藏 ↑", "收合已隱藏 ↑", "Collapse hidden ↑").to_string() } else { tr("显示 {0} 个已隐藏 ↓", "顯示 {0} 個已隱藏 ↓", "Show {0} hidden ↓").replace("{0}", &n.to_string()) };
        p.text(&s, rect(l, y + RULE, r, y + MORE - HAIR), &f.b13b, if hot { c.text } else { c.muted }, Align::L);
        p.fill(rect(0.0, y + MORE - HAIR, w, y + MORE), c.border);
        hit(b, Hit::More);
    }
    for &(i, ry) in &lay.rows {
        let e = &entries[i];
        let rr = row_rect(ry, w);
        // 选中（拿起的行、右键菜单对着的行）反色；悬停用 surface 并让左边的结构线变强调色
        let picked = a.drag.as_ref().is_some_and(|d| d.id == e.id);
        let selected = picked || a.menu_for == Some(e.id);
        let hot = a.hover == Some(Hit::Row(e.id)) || a.hover == Some(Hit::Login(e.id));
        let pal = if selected { th.on_selected() } else { *c };
        if picked {
            // 拿起的行盖在别的行上面：先铺不透明的底色
            p.fill(rr, D2D1_COLOR_F { a: 1.0, ..c.bg });
        }
        if selected {
            p.fill(rr, th.selected());
        } else if hot {
            p.fill(rr, th.surface());
        }
        p.fill(rect(0.0, ry, BAR, ry + ROW - HAIR), if selected { pal.accent } else if hot { c.accent } else { c.border });
        p.fill(rect(0.0, ry + ROW - HAIR, w, ry + ROW), c.border);
        // 已隐藏的一组退到 muted 色（对比度仍在 4.5:1 以上），不用半透明
        let dim = |col: D2D1_COLOR_F| if e.hidden { pal.muted } else { col };
        let left = e.otp.remaining(now);
        // 最后 5 秒变红（圆环和验证码），和手机上的验证器一样提醒快换码了；圆环里的数字说明剩几秒，不只靠颜色
        let urgent = left <= 5;
        let state = if urgent { pal.red } else { pal.accent };
        let cy = ry + (ROW - HAIR) / 2.0;
        p.ring(ring_cx, cy, RING_R - RING_W / 2.0, left as f32 / e.otp.period as f32, with_a(pal.muted, 0.35), state);
        p.text(&left.to_string(), rect(ring_cx - RING_R, cy - RING_R, ring_cx + RING_R, cy + RING_R), &f.l12b, if urgent { pal.red } else { pal.muted }, Align::C);
        // 开了登录按钮的，文字按钮放在验证码下面，和验证码右对齐
        let code_r = if e.login { rect(l, ry + U, code_end, ry + ROW - 4.0 * U) } else { rect(l, ry, code_end, ry + ROW - HAIR) };
        if e.login {
            let b = rect(code_end - 72.0, ry + ROW - 5.0 * U, code_end, ry + ROW - U);
            let on = a.hover == Some(Hit::Login(e.id));
            // 箭头在悬停时右移 2 px
            p.text(tr("登录 →", "登入 →", "Sign in →"), rect(b.left + if on { 2.0 } else { 0.0 }, b.top, b.right + if on { 2.0 } else { 0.0 }, b.bottom), &f.l12b, dim(if on && !selected { pal.text } else { pal.accent }), Align::R);
            hit(rect(b.left, b.top - U, b.right + 2.0, b.bottom + U), Hit::Login(e.id));
        }
        let cw = if a.copied == Some(e.id) {
            p.text(tr("已复制", "已複製", "Copied"), code_r, &f.s16b, pal.accent, Align::R)
        } else {
            p.text(&split_code(&e.otp.code(now)), code_r, &f.code, dim(if urgent { pal.red } else { pal.text }), Align::R)
        };
        let lr = (code_end - cw - 3.0 * U).max(l + 40.0);
        let editing = a.editing.filter(|&(id, _)| id == e.id).map(|(_, f)| f);
        // 填登录账号、密码时，名字下面一行提示在填什么
        let sub = match editing {
            Some(Field::User) => tr("输入游戏账号，回车下一步", "輸入遊戲帳號，Enter 下一步", "Game ID, Enter for next").to_string(),
            Some(Field::Pass) => tr("输入密码，回车保存", "輸入密碼，Enter 儲存", "Password, Enter to save").to_string(),
            _ if e.label == e.issuer || e.issuer.is_empty() => e.name.clone(),
            _ if e.name.is_empty() => e.issuer.clone(),
            _ => format!("{} · {}", e.issuer, e.name),
        };
        let sub = if sub == e.label { String::new() } else { sub };
        // 有副标题时名字在上、副标题在下，两行合起来对着行的中线；没有时名字对中线
        let nt = if sub.is_empty() { ry + (ROW - HAIR) / 2.0 - 10.0 } else { ry + 3.0 * U };
        if editing.is_none() {
            p.text(&e.label, rect(l, nt, lr, nt + 20.0), &f.b13b, dim(pal.text), Align::L);
        }
        p.text(&sub, rect(l, ry + 3.0 * U + 20.0, lr, ry + 3.0 * U + 38.0), &f.l12, dim(pal.muted), Align::L);
        hit(rr, Hit::Row(e.id));
    }
    unsafe {
        p.dc.SetTransform(&Matrix3x2::identity());
        p.dc.PopAxisAlignedClip();
    }
    Frame { h: top + lay.h + BOTTOM, list: (top, vis, lay.h), hits }
}

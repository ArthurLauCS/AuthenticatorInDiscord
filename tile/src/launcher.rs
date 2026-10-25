//! 一键登录台服 Lost Ark（MangoT5）启动器。启动器的登录界面是内嵌 IE 控件里的网页：账号密码在 loginIFrame，
//! 登录后弹出 OTP 页 otpFrame，过了 OTP 再点开始游戏。这里直接读写网页 DOM 填表、点页面自己的按钮，不模拟键盘，不受焦点、输入法影响。
//! 在后台线程跑，结果用 WM_APP_LOGIN 发回磁贴（lparam 是 Box<Result<String, String>>，两边都是给用户看的提示）。

use std::mem::ManuallyDrop;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use authcore::Otp;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::DISPID_PROPERTYPUT;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::ObjectFromLresult;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::WM_APP_LOGIN;
use crate::config::tr;

/// 台服安装包（Inno Setup）的卸载项，InstallLocation 是安装目录
const UNINSTALL_KEY: PCWSTR = w!("SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{5FC52390-E484-46EC-8256-138A38686927}_is1");
const IID_IHTMLDOCUMENT2: GUID = GUID::from_u128(0x332c4425_26cb_11d0_b483_00c04fd90119);

/// 用完自动 VariantClear 的 VARIANT；和 VARIANT 同布局，可以直接当参数数组传
#[repr(transparent)]
struct Var(VARIANT);

impl Drop for Var {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

impl Var {
    fn str(s: &str) -> Var {
        let mut v = VARIANT::default();
        unsafe {
            let v0 = &mut *v.Anonymous.Anonymous;
            v0.vt = VT_BSTR;
            v0.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(s));
        }
        Var(v)
    }

    fn vt(&self) -> VARENUM {
        unsafe { self.0.Anonymous.Anonymous.vt }
    }
}

/// 网页里的一个对象（IE 的 IDispatch）：按名字取属性、改属性、调方法，相当于 JS 里的 a.b、a.b = s、a.b(s)
struct Js(IDispatch);

impl Js {
    fn invoke(&self, name: &str, flags: DISPATCH_FLAGS, arg: Option<&str>) -> Option<Var> {
        let n = HSTRING::from(name);
        let mut id = 0;
        let mut args: Vec<Var> = arg.map(Var::str).into_iter().collect();
        let mut put = DISPID_PROPERTYPUT;
        let is_put = flags == DISPATCH_PROPERTYPUT;
        let params = DISPPARAMS { rgvarg: args.as_mut_ptr().cast(), rgdispidNamedArgs: if is_put { &mut put } else { std::ptr::null_mut() }, cArgs: args.len() as u32, cNamedArgs: is_put as u32 };
        let mut out = Var(VARIANT::default());
        unsafe {
            self.0.GetIDsOfNames(&GUID::zeroed(), &PCWSTR(n.as_ptr()), 1, 0, &mut id).ok()?;
            self.0.Invoke(id, &GUID::zeroed(), 0, flags, &params, Some(&mut out.0), None, None).ok()?;
        }
        Some(out)
    }

    fn obj(v: Var) -> Option<Js> {
        (v.vt() == VT_DISPATCH).then(|| unsafe { (*v.0.Anonymous.Anonymous.Anonymous.pdispVal).clone() }).flatten().map(Js)
    }

    /// a.name，是对象时
    fn get(&self, name: &str) -> Option<Js> {
        Js::obj(self.invoke(name, DISPATCH_PROPERTYGET, None)?)
    }

    /// a.name，是字符串时；别的类型当空串
    fn text(&self, name: &str) -> String {
        self.invoke(name, DISPATCH_PROPERTYGET, None).filter(|v| v.vt() == VT_BSTR).map(|v| unsafe { v.0.Anonymous.Anonymous.Anonymous.bstrVal.to_string() }).unwrap_or_default()
    }

    fn set(&self, name: &str, s: &str) -> bool {
        self.invoke(name, DISPATCH_PROPERTYPUT, Some(s)).is_some()
    }

    /// 点一下按钮，触发页面自己的 onclick
    fn click(&self) -> bool {
        self.invoke("click", DISPATCH_METHOD, None).is_some()
    }

    /// document.getElementById(id)
    fn el(&self, id: &str) -> Option<Js> {
        Js::obj(self.invoke("getElementById", DISPATCH_METHOD, Some(id))?)
    }

    /// document.querySelector(sel)
    fn one(&self, sel: &str) -> Option<Js> {
        Js::obj(self.invoke("querySelector", DISPATCH_METHOD, Some(sel))?)
    }

    /// 第一个匹配 sel 的元素显示着（jQuery 的 show / hide 改的是行内 display）
    fn shown(&self, sel: &str) -> bool {
        self.one(sel).and_then(|e| e.get("style")).is_some_and(|s| s.text("display") != "none")
    }

    /// 页面里的提示文字，去掉开头的“*”
    fn msg(&self, id: &str) -> String {
        self.el(id).map(|e| e.text("innerText")).unwrap_or_default().trim().trim_start_matches('*').trim().to_string()
    }

    /// 加载完的 iframe 的 document。用 contentDocument：从外部进程取 https 页面的 contentWindow.document 得到 null
    fn frame(&self, id: &str) -> Option<Js> {
        let doc = self.el(id)?.get("contentDocument")?;
        (doc.text("readyState") == "complete").then_some(doc)
    }
}

unsafe extern "system" fn each_top(h: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        let _ = EnumChildWindows(Some(h), Some(each_child), lp);
    }
    TRUE
}

unsafe extern "system" fn each_child(h: HWND, lp: LPARAM) -> BOOL {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(h, &mut buf) } as usize;
    if String::from_utf16_lossy(&buf[..n]) == "Internet Explorer_Server" {
        unsafe { (*(lp.0 as *mut Vec<HWND>)).push(h) };
    }
    TRUE
}

/// 找启动器：哪个 IE 控件打开着带 loginIFrame 的页面。返回 (顶层窗口, 页面 document)
fn find() -> Option<(HWND, Js)> {
    let mut ies: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each_top), LPARAM(&mut ies as *mut _ as isize));
        let msg = RegisterWindowMessageW(w!("WM_HTML_GETOBJECT"));
        ies.into_iter().find_map(|h| {
            let mut res = 0usize;
            if SendMessageTimeoutW(h, msg, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 1000, Some(&mut res)).0 == 0 {
                return None;
            }
            let mut p = std::ptr::null_mut();
            ObjectFromLresult(LRESULT(res as isize), &IID_IHTMLDOCUMENT2, WPARAM(0), &mut p).ok()?;
            // IHTMLDocument2 派生自 IDispatch，按 IDispatch 用
            let doc = Js(IDispatch::from_raw(p));
            doc.el("loginIFrame").is_some().then(|| (GetAncestor(h, GA_ROOT), doc))
        })
    }
}

fn launcher_exe() -> Option<PathBuf> {
    let mut buf = [0u16; 520];
    let mut n = size_of_val(&buf) as u32;
    let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, UNINSTALL_KEY, w!("InstallLocation"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut n)) };
    let dir = String::from_utf16_lossy(&buf[..(n as usize / 2).saturating_sub(1)]);
    let exe = PathBuf::from(dir).join("Launcher.exe");
    (r == ERROR_SUCCESS && exe.exists()).then_some(exe)
}

/// 每 0.3 秒试一次 f，最多等 secs 秒
fn wait<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() > end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn run(user: &str, pass: &str, otp: &Otp) -> std::result::Result<String, String> {
    let late = || tr("启动器没有响应，请重试", "啟動器沒有回應，請重試", "The launcher didn't respond; try again").to_string();
    let (top, doc) = match find() {
        Some(f) => f,
        None => {
            let exe = launcher_exe().ok_or(tr("找不到 Lost Ark 启动器", "找不到 Lost Ark 啟動器", "Lost Ark launcher not found"))?;
            std::process::Command::new(&exe).current_dir(exe.parent().unwrap_or(&exe)).spawn().map_err(|_| tr("打不开 Lost Ark 启动器", "無法開啟 Lost Ark 啟動器", "Can't start the Lost Ark launcher"))?;
            wait(60, find).ok_or_else(late)?
        }
    };
    // 已经过了账号密码这一步（OTP 页或登录后的主界面），可能是别的账号，不去动它
    if !doc.shown("#loginScreen") || doc.shown("#otpScreen") {
        return Err(tr("启动器已登录，请先退出", "啟動器已登入，請先登出", "The launcher is already signed in; sign out first").into());
    }
    let changed = || tr("启动器的登录页变了，没法自动填", "啟動器的登入頁變了，無法自動填入", "The launcher's sign-in page changed; can't fill it").to_string();
    let form = wait(30, || doc.frame("loginIFrame")).ok_or_else(late)?;
    // 页面用同步请求登录，点“登入”返回时结果已经出来：账号密码错了会写在 errorMsg 里
    if !(form.el("userID").is_some_and(|e| e.set("value", user)) && form.el("userPW").is_some_and(|e| e.set("value", pass)) && form.one(".btn-submit").is_some_and(|b| b.click())) {
        return Err(changed());
    }
    let err = form.msg("errorMsg");
    if !err.is_empty() {
        return Err(err);
    }
    if doc.shown("#otpScreen") {
        let od = wait(15, || doc.frame("otpFrame").filter(|d| d.el("otp1").is_some())).ok_or_else(late)?;
        // 快换码时等下一组，免得刚填进去就过期
        while otp.remaining(now()) < 3 {
            std::thread::sleep(Duration::from_millis(500));
        }
        for (i, c) in otp.code(now()).chars().enumerate() {
            if let Some(e) = od.el(&format!("otp{}", i + 1)) {
                e.set("value", &c.to_string());
            }
        }
        if !od.el("lock").is_some_and(|b| b.click()) {
            return Err(changed());
        }
        // 验证是异步请求：OTP 页关掉就是过了，错了写在 errorMsgOtp 里
        wait(15, || if !doc.shown("#otpScreen") { Some(Ok(())) } else { Some(od.msg("errorMsgOtp")).filter(|m| !m.is_empty()).map(Err) }).ok_or_else(late)??;
    }
    // 登录后启动器检查游戏版本：开始按钮先是 WAIT，查完变成 PLAY / UPDATE / REPAIR / ERROR；维护时换成“維護中”按钮
    let btn = doc.el("playButton").ok_or_else(changed)?;
    let state = wait(60, || if doc.shown(".btn-maintain") { Some("MAINTAIN".into()) } else { Some(btn.text("innerText")).filter(|t| ["PLAY", "UPDATE", "REPAIR", "ERROR"].contains(&t.as_str())) });
    let r = match state.as_deref() {
        Some("PLAY") if btn.click() => Ok(tr("已登录，正在启动游戏", "已登入，正在啟動遊戲", "Signed in; starting the game").into()),
        // 先勾上启动器自带的“自動開啟遊戲”再点更新，更新完启动器自己开游戏
        Some("UPDATE") if (doc.one("#autoStart:checked").is_some() || doc.el("autoStart").is_some_and(|c| c.click())) && btn.click() => {
            Ok(tr("正在更新游戏，更新完自动开始", "正在更新遊戲，更新完自動開始", "Updating the game; it starts when done").into())
        }
        Some("PLAY" | "UPDATE") => Err(changed()),
        Some("REPAIR") => Err(tr("游戏文件要修复，请在启动器里处理", "遊戲檔案需要修復，請在啟動器裡處理", "The game files need repair; fix them in the launcher").into()),
        Some("ERROR") => Err(tr("启动器检查版本出错，请看启动器里的提示", "啟動器檢查版本出錯，請看啟動器裡的提示", "The launcher's version check failed; see the launcher").into()),
        Some(_) => Err(tr("服务器维护中", "伺服器維護中", "The servers are under maintenance").into()),
        None => Err(tr("启动器还在检查版本，请稍后在启动器里开始游戏", "啟動器還在檢查版本，請稍後在啟動器裡開始遊戲", "The launcher is still checking the game version; start the game there").into()),
    };
    // 把启动器拿到前面：看启动和更新进度，或处理上面的提示
    unsafe {
        if IsIconic(top).as_bool() {
            let _ = ShowWindow(top, SW_RESTORE);
        }
        let _ = SetForegroundWindow(top);
    }
    r
}

/// 后台登录，完成后给 hwnd 发 WM_APP_LOGIN
pub fn start(hwnd: HWND, user: String, pass: String, otp: Otp) {
    let hw = hwnd.0 as usize;
    std::thread::spawn(move || {
        let r = match unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok() {
            Ok(()) => {
                let r = run(&user, &pass, &otp);
                unsafe { CoUninitialize() };
                r
            }
            Err(e) => Err(e.message()),
        };
        unsafe {
            let p = Box::into_raw(Box::new(r));
            if PostMessageW(Some(HWND(hw as _)), WM_APP_LOGIN, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
            }
        }
    });
}

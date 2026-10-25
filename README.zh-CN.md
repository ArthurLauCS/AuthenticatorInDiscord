# AuthTile

[English](README.md) | 简体中文

Windows 桌面磁贴，实时显示 Google 验证器（Google Authenticator）的验证码。导入 Google 验证器导出的二维码，每个账号一行，显示当前验证码和倒计时环。

- 单个约 1.2 MB 的 exe。私有内存约 6 MB；Windows 把输入法载入进程后约 40 MB。
- 跟随系统浅色 / 深色主题；透明度可调，最低可以几乎透明。
- 简体中文、繁体中文、英文界面。

## 安装

从 [Releases](https://github.com/ArthurLauCS/AuthenticatorInDiscord/releases/latest) 下载 `AuthTile-Setup-<版本>.exe` 并运行。需要 64 位 Windows 10 或 11。

- 安装包没有商业代码签名证书，Windows 可能提示“Windows 已保护你的电脑”，点**更多信息** → **仍要运行**。
- 只为当前用户安装，不需要管理员权限。
- 安装时选的语言就是界面语言，之后可以在菜单里改。

## 导入账号

在 Google 验证器里点**转移账号 → 导出账号**，把显示出来的二维码截图。如果手机不让截这个页面，就用另一台设备拍下来。

然后用下面任意一种方式交给磁贴：

- 把一张或几张图片文件拖到磁贴上。
- 点磁贴顶部的 **+**，选择图片。
- 复制图片，或在资源管理器里复制图片文件，在磁贴上按 **Ctrl+V**（也可以在菜单里点**从剪贴板导入**）。

支持 PNG、JPG、WebP、BMP，一张图里可以有好几个二维码。标准的 `otpauth://totp/...` 二维码和复制的 `otpauth://` 链接也能导入。磁贴上已有的账号会跳过；不支持 HOTP（计数器型）账号。导入后会提示导入、跳过、不支持各几个。

## 使用

- **点一行**复制它的验证码。复制的验证码不会进 Windows 剪贴板历史（Win+V）。
- 验证码旁边的环表示剩余时间，最后 5 秒变红。
- **右键一行**：复制、重命名（可以带 emoji）、上移、下移、隐藏、设置游戏登录账号、删除。删除要再点一次确认。
- **拖动一行**调整顺序。
- 隐藏的验证码不会删掉，收在底部的**显示 N 个已隐藏**按钮里。点它在按钮下方变淡显示，照样可以复制、拖动排序和取消隐藏。磁贴启动时是收起的。
- **右键磁贴其他地方或托盘图标**打开设置：透明度、置顶显示、开机自启、语言、导入、隐藏磁贴、退出。
- 拖动磁贴顶部的标题栏可以移动位置。顶部的图钉按钮切换置顶。左键点托盘图标隐藏或显示磁贴。

## 一键登录 Lost Ark（台服）

玩台服 Lost Ark（MangoT5 启动器）时，可以让某一行帮你登录并开始游戏：磁贴替你在启动器里填账号、密码和 OTP 验证码，再点开始游戏。

1. 右键游戏账号绑定的那一行验证码，点**设置游戏登录账号…**。输入账号，按回车；再输入密码，按回车。
2. 再右键这一行，打开**登录按钮**。验证码和倒计时环之间会出现一个手柄按钮。
3. 点这个按钮。启动器没开时磁贴会先打开它，然后填账号密码、点登入、在 OTP 页填验证码并确认，等启动器检查完游戏版本后点开始游戏，最后把启动器拿到前面。

- 只有设置过的行才有按钮，其他验证码不受影响。
- 启动器已经登录，或者停在 OTP 页时，磁贴不去动它，提示你先退出，免得接着别的账号的登录往下走。
- 密码或验证码不对时，磁贴上显示启动器自己的错误提示。
- 当前验证码剩不到 3 秒时，等下一组码再填。
- 游戏要更新时，磁贴勾上启动器自带的“自動開啟遊戲”再点更新，更新完启动器自己开游戏。启动器要求修复、报错或显示维护中时，磁贴只提示你，交给你处理。
- 改密码：再点一次**设置游戏登录账号…**，密码框留空按回车就保留原密码。取消登录：把账号清空后按回车。
- 磁贴只操作启动器的网页，不碰游戏本身。

## 数据

账号存在 `%APPDATA%\AuthTile\vault.dat`，用 Windows DPAPI 按当前 Windows 用户加密。只有同一台电脑上的同一个 Windows 用户能打开，把 `vault.dat` 复制到别处也没用；换电脑时重新导入二维码。请把导出的二维码图片存在安全的地方，或者在 Google 验证器里也保留这些账号。

能使用你已解锁的 Windows 账户的人都能看到验证码，和手机解锁后打开验证器一样。

游戏登录账号和密码存在同一个文件里，加密方式相同。密码和 OTP 密钥放在一起，所以能使用你已解锁的 Windows 账户的人也能登录这些游戏账号：在这台电脑上，双重验证不再起作用。密码在别处泄露时，没有验证码照样登不进去。

卸载时会问要不要同时删除设置和账号，默认保留。

## 从源码构建

需要 Rust（stable，MSVC 工具链）。

```powershell
cargo test --workspace                  # TOTP、二维码识别、导入去重、设置
cargo build --release -p authtile       # target\release\authtile.exe
```

- `core`（crate 名 `authcore`）是不带界面的引擎：RFC 6238 的 TOTP（SHA-1 / SHA-256 / SHA-512，6 或 8 位）、二维码识别、`otpauth://` 和 `otpauth-migration://` 解析、账号列表和导入去重。
- `tile` 是 Windows 程序：Direct2D 画在 DirectComposition 交换链上，亚克力背景。`tile\src\launcher.rs` 读写 Lost Ark 启动器内嵌 IE 控件里网页的 DOM，完成登录。
- `scripts\installer.ps1` 把安装包打到 `target\installer`（需要 [Inno Setup 6](https://jrsoftware.org/isinfo.php)），只保留当前版本的安装包。
- `scripts\icon.py` 把 `tile\assets\icon.svg` / `icon-small.svg` 渲染成 `app.ico` 和 `app.res`（需要装了 Pillow 的 Python 和 Google Chrome）。
- `cargo run -p authcore --example mkqr -- out.png "发行方|账号|BASE32密钥"` 生成假账号的测试二维码。

Google 验证器导出格式参照 `proto/googleauth.proto`，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

# AuthTile

[English](README.md) | 简体中文

Windows 桌面磁贴，实时显示 Google 验证器（Google Authenticator）的验证码。导入 Google 验证器导出的二维码，每个账号一行，显示当前验证码和倒计时环。

- 单个约 1.2 MB 的 exe。私有内存约 6 MB；Windows 把输入法载入进程后约 40 MB。
- 跟随系统浅色 / 深色主题；透明度可调，最低可以几乎透明。
- 中文、英文界面。

## 安装

运行 `AuthTile-Setup-<版本>.exe`。需要 64 位 Windows 10 或 11。

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
- **右键一行**：复制、重命名（可以带 emoji）、上移、下移、删除。删除要再点一次确认。
- **右键磁贴其他地方或托盘图标**打开设置：透明度、置顶显示、开机自启、语言、导入、隐藏磁贴、退出。
- 拖动磁贴可以移动位置。顶部的图钉按钮切换置顶。左键点托盘图标隐藏或显示磁贴。

## 数据

账号存在 `%APPDATA%\AuthTile\vault.dat`，用 Windows DPAPI 按当前 Windows 用户加密。只有同一台电脑上的同一个 Windows 用户能打开，把 `vault.dat` 复制到别处也没用；换电脑时重新导入二维码。请把导出的二维码图片存在安全的地方，或者在 Google 验证器里也保留这些账号。

能使用你已解锁的 Windows 账户的人都能看到验证码，和手机解锁后打开验证器一样。

卸载时会问要不要同时删除设置和账号，默认保留。

## 从源码构建

需要 Rust（stable，MSVC 工具链）。

```powershell
cargo test --workspace                  # TOTP、二维码识别、导入去重、设置
cargo build --release -p authtile       # target\release\authtile.exe
```

- `core`（crate 名 `authcore`）是不带界面的引擎：RFC 6238 的 TOTP（SHA-1 / SHA-256 / SHA-512，6 或 8 位）、二维码识别、`otpauth://` 和 `otpauth-migration://` 解析、账号列表和导入去重。
- `tile` 是 Windows 程序：Direct2D 画在 DirectComposition 交换链上，亚克力背景。
- `scripts\installer.ps1` 把安装包打到 `target\installer`（需要 [Inno Setup 6](https://jrsoftware.org/isinfo.php)），只保留当前版本的安装包。
- `scripts\icon.py` 把 `tile\assets\icon.svg` / `icon-small.svg` 渲染成 `app.ico` 和 `app.res`（需要装了 Pillow 的 Python 和 Google Chrome）。
- `cargo run -p authcore --example mkqr -- out.png "发行方|账号|BASE32密钥"` 生成假账号的测试二维码。

Google 验证器导出格式参照 `proto/googleauth.proto`，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

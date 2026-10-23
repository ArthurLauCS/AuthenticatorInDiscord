# Authenticator in Discord

在 Discord 中管理带备注的 TOTP 验证器，点击条目即可获取当前验证码。可在 Windows 本机运行，也可部署到 Linux VPS 长期运行。

```text
Google Authenticator 导出链接 / 原始 TOTP 密钥
                  ↓ 本机隐藏输入
          加密 SQLite 数据库
                  ↓
Discord /auth → 名称、备注、条目按钮 → 仅自己可见的验证码
```

## 已有功能

- `/auth` 私密面板：每页 10 条，按按钮取码，显示到期时间。
- 名称、备注可在 Discord 中修改；删除需二次确认。
- 原始 Base32 密钥、标准 `otpauth://totp/…` 链接导入。
- Google Authenticator `otpauth-migration://offline?data=…` 批量导入，验证完整批次，重复密钥跳过。
- SHA1 / SHA256 / SHA512、6 / 8 位验证码；单条 otpauth 链接支持 15–120 秒周期，Google 迁移按其格式使用 30 秒。
- Discord 用户 ID 白名单；多用户各自拥有独立条目。每次按钮、表单和删除操作均检查用户权限。
- 密钥、名称与备注一起使用 Fernet 加密；SQLite 中只有随机条目 ID、所属用户 ID 和密文。
- 断线重连、可选 HTTP 代理、Docker 自动重启、systemd 服务及一致性备份命令。

Google 迁移协议定义复用自 [JeaNile/gotp](https://github.com/JeaNile/gotp/tree/f49ac7fd5e2445e62f1c6e6e3b449925ba685c2f)。原项目提供导出解析和命令行取码，本项目补充 Discord 界面、权限与加密持久化。许可证及来源见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。无需安装 Go 或 gotp。

## 1. 创建 Discord Bot

1. 打开 [Discord Developer Portal](https://discord.com/developers/applications)，创建应用。
2. 在 **Bot** 页面获取 Bot Token。只在本机初始化提示或 `.env` 中填写，不要发送到聊天或提交到 Git。
3. 在 **OAuth2 → URL Generator** 中选择 `bot` 和 `applications.commands`，通过生成的链接将 Bot 加入自己的服务器。无需 Administrator，也无需开启 Message Content、Members 等特权 Intents。
4. 在 Discord **用户设置 → 高级 → 开发者模式** 中启用开发者模式；右键自己的用户头像复制 **用户 ID**。不是用户名，也不是 Bot ID。
5. 可选：复制目标服务器 ID 写入 `DISCORD_GUILD_ID`，使 `/auth` 注册到该服务器；留空注册为全局命令。首次注册后 Discord 客户端可能需要稍等或刷新。

本项目通过 Gateway 主动连接 Discord，无需域名、HTTPS 证书、入站端口或 Interactions Endpoint URL。服务器频道需允许你使用应用命令。

## 2. Windows 本机启动

需要 Python 3.11+，本项目已在 Python 3.14 上验证。进入项目目录后执行：

```powershell
./setup.ps1
./start.ps1
```

`setup.ps1` 安装依赖，并交互询问 Token 和用户 ID，自动生成加密密钥；不会覆盖已有 `.env`。如 PowerShell 执行策略阻止脚本，可直接执行等价命令：

```powershell
python -m venv .venv
./.venv/Scripts/python.exe -m pip install -r requirements.txt
./.venv/Scripts/python.exe -m authbot setup
./.venv/Scripts/python.exe -m authbot run
```

运行期间保持终端和电脑开启；按 Ctrl+C 停止。本机休眠、断网或关闭进程后 Bot 无法响应。

### 导入单条验证器

另开一个终端，进入相同项目目录：

```powershell
./.venv/Scripts/python.exe -m authbot add
```

按提示输入名称、备注和原始 Base32 密钥或 `otpauth://totp/` 链接。密钥输入不会回显，也不会放入命令行参数或 shell 历史。**当前的 6 位验证码不能用于导入**。

### 从 Google Authenticator 批量迁移

1. 在手机 Google Authenticator 选择转移 / 导出账号，勾选需要导入的账号。
2. 使用可信的离线二维码读取工具取得二维码原文，形式为 `otpauth-migration://offline?data=…`。不要上传到在线解码网站。本项目接受链接文本，不直接读取二维码图片。
3. 执行以下命令，逐条粘贴本次导出的所有二维码链接；输入不回显，最后再按一次空回车结束。

```powershell
./.venv/Scripts/python.exe -m authbot import-google
```

同一批次的二维码必须全部提供；不完整、重复二维码、混合批次或含不支持的 HOTP / 算法时整次拒绝，不会部分写入。再次导入相同密钥、算法、位数和周期会跳过，保留已有名称与备注。

导入不会删除手机上的账号，也不与手机持续同步。新增账号需再次导入。Google 账号密码、Google 云同步登录均不需要。

### 在 Discord 取码和备注

1. 输入 `/auth`，得到仅自己可见的列表。
2. 点击条目按钮，获取验证码和到期时间；即将过期的码会等待下一周期再显示。
3. 在验证码卡片中点击「修改名称 / 备注」或「删除条目」。编辑成功会打开更新后的列表；旧列表可点「刷新列表」。
4. 需要新验证码时再次点击条目。面板 10 分钟后或服务重启后失效，重新输入 `/auth` 即可。

默认通过本机导入密钥。如果希望直接在 Discord 表单中添加，将 `.env` 中 `ALLOW_DISCORD_IMPORT=true` 并重启。开启后密钥会经过 Discord 的服务，表单不是端到端加密通道；Google 批量导入仍走本机命令。

## 配置

环境变量优先于项目根目录 `.env`。相对 `DATA_DIR` 相对于项目根目录解析，不受启动位置影响。

| 配置 | 用途 |
| --- | --- |
| `DISCORD_TOKEN` | Bot Token，启动 Bot 时必填 |
| `ALLOWED_USER_IDS` | 允许访问的数字用户 ID，以英文逗号分隔，必填 |
| `ENCRYPTION_KEY` | 初始化自动生成，必须保存原值；丢失无法解密 |
| `DATA_DIR` | SQLite 保存目录，默认 `./data` |
| `DISCORD_GUILD_ID` | 可选，只注册到指定服务器 |
| `DISCORD_PROXY` | 可选，例如 `http://127.0.0.1:7897`，同时用于 REST 和 Gateway |
| `ALLOW_DISCORD_IMPORT` | 默认 `false`，是否允许在 Discord 表单中输入密钥 |

代理地址需按实际环境填写；本项目不会修改电脑代理配置。容器内的 `127.0.0.1` 指容器自身，不能直接用来访问宿主机代理。

配置多个允许用户时，CLI 的 `add`、`import-google`、`list` 必须用 `--user 用户ID` 指定归属。修改配置后重启服务生效。

```powershell
./.venv/Scripts/python.exe -m authbot check
./.venv/Scripts/python.exe -m authbot list --user 123456789012345678
```

`check` 仅离线验证配置与数据库解密，不验证 Discord 登录；`list` 仅输出名称和备注。启动和命令错误不输出密钥、Token 或原始异常堆栈。

## 3. VPS：Docker Compose

VPS 安装 Docker 和 Compose 插件后，将源文件上传到独立目录。保持服务器系统时间同步。无需开放任何入站端口。

首次部署可在本机运行 `setup` 生成 `.env`，然后通过可信渠道复制到 VPS，执行 `chmod 600 .env`；也可在 VPS 用 Python 安装依赖后运行 `python -m authbot setup`。已有数据库必须配套使用原加密密钥。

```bash
docker compose up -d --build
docker compose logs --tail=50 authbot
docker compose exec authbot python -m authbot check
docker compose exec authbot python -m authbot import-google
```

交互导入使用 `docker compose exec`，保持交互终端。数据保存在命名卷 `vault` 中，与本机 `data/` **不是同一份数据**。首次部署可重新导入，或按下文迁移备份。

容器使用非 root 用户、只读应用文件系统和可写数据卷；`restart: unless-stopped` 在 Docker 服务启动后恢复。启用 VPS 的 Docker 开机启动。停止用 `docker compose stop`；更新用 `docker compose up -d --build`。不要使用 `docker compose down -v`，它会删除数据卷。不要同时启动两个相同 Token 的服务实例。

### Docker 备份与迁移

```bash
docker compose exec authbot python -m authbot backup /app/data/backup-2026-10-23.db
docker compose cp authbot:/app/data/backup-2026-10-23.db ./backup-2026-10-23.db
chmod 600 ./backup-2026-10-23.db
```

备份文件名必须不存在。另行备份原 `.env` 或 `ENCRYPTION_KEY`，并与数据库备份分开安全保存。

迁移本机数据到全新 Docker 卷：先创建本机备份并安全复制到 VPS，同时使用原 `.env`。在尚无旧数据的部署目录执行：

```bash
docker compose build
docker compose create
docker compose cp ./backup.db authbot:/app/data/vault.db
docker compose run --rm --user 0 --entrypoint sh authbot -c 'chown 10001:10001 /app/data/vault.db && chmod 600 /app/data/vault.db'
docker compose run --rm authbot python -m authbot check
docker compose up -d
```

向已有卷恢复时，先停止服务并备份当前数据库，再替换；不要直接覆盖运行中的数据库。Windows 与 Linux 可以使用同一份数据库备份及原加密密钥。

## VPS：不使用 Docker

将项目放到 `/opt/authbot`，安装 Python 3.11+ 和 venv，创建专用系统用户。以下以已将源码复制到该目录的 Debian / Ubuntu 主机为例：

```bash
sudo useradd --system --home /opt/authbot --shell /usr/sbin/nologin authbot
cd /opt/authbot
sudo python3 -m venv .venv
sudo .venv/bin/python -m pip install -r requirements.txt
sudo .venv/bin/python -m authbot setup
sudo install -d -m 700 -o authbot -g authbot /opt/authbot/data
sudo chown authbot:authbot .env
sudo chmod 600 .env
sudo cp deploy/authbot.service /etc/systemd/system/authbot.service
sudo systemctl daemon-reload
sudo systemctl enable --now authbot
sudo journalctl -u authbot -n 50 --no-pager
```

运行交互导入：`sudo -u authbot /opt/authbot/.venv/bin/python -m authbot import-google`。停止服务：`sudo systemctl stop authbot`。如果专用用户已经存在，不需重复执行 `useradd`。

## 备份、安全与边界

```powershell
./.venv/Scripts/python.exe -m authbot backup ./backups/vault-backup.db
```

- 备份使用 SQLite 在线备份接口，可在 Bot 运行时执行；恢复必须停止服务，并使用同一 `ENCRYPTION_KEY`。
- `.env`、数据库和备份已加入 `.gitignore`，Docker 构建上下文也不包含它们。Windows 上请把项目与备份放在仅自己可访问的目录；Linux 上默认新建数据文件权限为 600。
- 验证码通过 Discord 传输，私密回复仅对请求用户可见，但不意味着 Discord 平台无法处理该内容。Discord 账号或运行主机被攻破，可能影响这里托管的验证器。不要把 Discord 自身的唯一二次验证来源只放在此 Bot 中。
- 验证码卡片到期后尝试删除；进程中断、断网或 Discord API 拒绝时删除可能失败，卡片仍标有到期时间。保留显示的过期码不再代表当前有效码。
- 加密保护离线数据库副本，不能防止同时获得数据库和 `.env` 的攻击者。备份密钥必须保留，不能靠重新运行 setup 恢复旧数据。
- 仅支持标准 TOTP，不支持 HOTP、Steam、短信验证码、直接读取 Google 云同步、自动扫码图片或浏览器自动登录。

## 开发与验证

```powershell
./.venv/Scripts/python.exe -m unittest discover -s tests -v
./.venv/Scripts/python.exe -m compileall -q authbot tests
```

测试覆盖 RFC 6238 三种算法向量、周期边界、迁移批次检查、CLI 导入、密文存储、重启恢复、错误密钥、用户隔离、密文调换检测、备份恢复、交互权限、按钮取码、分页和确认删除。测试使用临时数据库与模拟 Discord 交互，不连接真实账号。

仅修改迁移协议时需要安装 `requirements-dev.txt` 并按 `THIRD_PARTY_NOTICES.md` 重新生成 Python protobuf 文件。运行时不需要编译器。

参考：[Google Key URI 格式](https://github.com/google/google-authenticator/wiki/Key-Uri-Format)、[PyOTP](https://pyauth.github.io/pyotp/)、[discord.py 交互 API](https://discordpy.readthedocs.io/en/stable/interactions/api.html)、[Fernet](https://cryptography.io/en/latest/fernet/)。

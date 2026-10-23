import argparse
import getpass
import logging
import os
import sys
import warnings
from pathlib import Path

from cryptography.fernet import Fernet
from .config import ROOT, load_settings
from .migration import parse_migration
from .otp import parse_secret
from .store import Store


def hidden_input(prompt):
    # Do not let getpass fall back to echoing secrets in redirected terminals.
    with warnings.catch_warnings():
        warnings.simplefilter("error", getpass.GetPassWarning)
        try:
            return getpass.getpass(prompt)
        except getpass.GetPassWarning:
            raise ValueError("请在交互终端中运行，当前终端不能隐藏密钥输入。") from None


def setup():
    destination = ROOT / ".env"
    if destination.exists():
        raise ValueError(".env 已存在，未作修改；请直接编辑现有配置，保留原加密密钥。")
    token = hidden_input("Discord Bot Token（不回显，可留空稍后填写）: ").strip()
    users = input("允许使用的 Discord 用户 ID（多个用逗号分隔）: ").strip()
    if not users or any(not x.strip().isdigit() or not 0 < int(x.strip()) < 2**64 for x in users.split(",")):
        raise ValueError("用户 ID 必须为正整数。")
    if any(c in token for c in "\r\n'\\"):
        raise ValueError("Token 格式无效。")
    content = (f"DISCORD_TOKEN='{token}'\nALLOWED_USER_IDS={users}\n"
               f"ENCRYPTION_KEY={Fernet.generate_key().decode()}\nDATA_DIR=./data\n"
               "DISCORD_GUILD_ID=\nDISCORD_PROXY=\nALLOW_DISCORD_IMPORT=false\n")
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(content)
    print("配置已保存到 .env。请妥善备份；ENCRYPTION_KEY 丢失后无法解密已有数据。")


def main():
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description="Discord 私人验证器；敏感信息只通过隐藏输入读取。")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("setup", help="初始化本地 .env，不覆盖现有文件")
    commands.add_parser("run", help="启动 Discord Bot")
    commands.add_parser("check", help="离线检查配置和数据库，不连接 Discord")
    for name, help_text in [("add", "导入一条 Base32 密钥或 otpauth 链接"),
                            ("import-google", "批量导入 Google Authenticator 导出链接"),
                            ("list", "列出条目名称与备注，不输出密钥和验证码")]:
        command = commands.add_parser(name, help=help_text)
        command.add_argument("--user", type=int, help="条目所属用户 ID；只有一个允许用户时可省略")
    backup = commands.add_parser("backup", help="创建数据库一致性备份；需另外安全备份 .env")
    backup.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "setup":
            setup()
            return 0
        settings = load_settings(require_token=args.command == "run")
        store = Store(settings.data_dir / "vault.db", settings.key)
        if args.command == "run":
            from .bot import AuthBot
            logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
            # Library HTTP errors can include request details; keep only our sanitized logs.
            logging.getLogger("discord").setLevel(logging.CRITICAL)
            AuthBot(settings, store).run(settings.token, log_handler=None)
        elif args.command == "check":
            total = sum(len(store.list(user)) for user in settings.allowed_users)
            print(f"离线检查通过：{len(settings.allowed_users)} 个允许用户，{total} 条可解密记录。")
            print("Bot Token 已填写。" if settings.token else "Bot Token 未填写；启动前需配置。")
        elif args.command == "backup":
            store.backup(args.destination)
            print("数据库备份已创建；请另外安全保存原 .env / ENCRYPTION_KEY。")
        else:
            owner = args.user
            if owner is None and len(settings.allowed_users) == 1:
                owner = next(iter(settings.allowed_users))
            if owner not in settings.allowed_users:
                raise ValueError("请用 --user 指定 ALLOWED_USER_IDS 中的一个用户 ID。")
            if args.command == "add":
                label = input("名称: ")
                note = input("备注（可留空）: ")
                otp = parse_secret(hidden_input("Base32 密钥或 otpauth://totp 链接（不回显）: "))
                store.add(owner, label, note, otp)
                print("已添加，在 Discord 输入 /auth 或刷新列表即可查看。")
            elif args.command == "import-google":
                print("逐条粘贴同一批次的所有迁移二维码链接；输入不回显，空行结束。")
                uris = []
                while True:
                    uri = hidden_input(f"导出链接 {len(uris) + 1}: ").strip()
                    if not uri:
                        break
                    uris.append(uri)
                    if len(uris) > 100:
                        raise ValueError("每次最多导入 100 个二维码链接。")
                entries = parse_migration(uris)
                added, skipped = store.import_many(owner, entries)
                print(f"导入完成：新增 {added} 条，跳过 {skipped} 条重复密钥。可在 Discord 中修改名称和备注。")
            else:
                for entry in store.list(owner):
                    # repr escapes terminal control characters in imported account names.
                    print(f"{entry.id}  {entry.label!r}  {entry.note!r}")
        return 0
    except (KeyboardInterrupt, EOFError):
        print("已退出。", file=sys.stderr)
        return 130
    except ValueError as error:
        print(f"错误：{error}", file=sys.stderr)
        return 1
    except Exception as error:
        print(f"操作失败（{type(error).__name__}）；请检查配置、网络、文件权限或数据库。", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

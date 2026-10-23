import os
from dataclasses import dataclass, field
from pathlib import Path
from urllib.parse import urlparse

from cryptography.fernet import Fernet
from dotenv import load_dotenv

ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Settings:
    token: str = field(repr=False)
    allowed_users: frozenset[int]
    key: str = field(repr=False)
    data_dir: Path
    guild_id: int | None = None
    proxy: str | None = field(default=None, repr=False)
    discord_import: bool = False


def load_settings(require_token=False):
    load_dotenv(ROOT / ".env", interpolate=False)
    try:
        allowed = frozenset(int(x.strip()) for x in os.getenv("ALLOWED_USER_IDS", "").split(",") if x.strip())
        if not allowed or any(x <= 0 or x >= 2**64 for x in allowed):
            raise ValueError
        guild = int(os.environ["DISCORD_GUILD_ID"]) if os.getenv("DISCORD_GUILD_ID") else None
        if guild is not None and not 0 < guild < 2**64:
            raise ValueError
    except ValueError:
        raise ValueError("请配置有效的 ALLOWED_USER_IDS / DISCORD_GUILD_ID 数字 ID。") from None
    key = os.getenv("ENCRYPTION_KEY", "")
    try:
        Fernet(key.encode("ascii"))
    except Exception:
        raise ValueError("ENCRYPTION_KEY 无效；首次使用请运行 setup，已有数据请恢复原密钥。") from None
    token = os.getenv("DISCORD_TOKEN", "").strip()
    if require_token and not token:
        raise ValueError("未配置 DISCORD_TOKEN，请在本机 .env 文件中填写。")
    proxy = os.getenv("DISCORD_PROXY", "").strip() or None
    if proxy and (urlparse(proxy).scheme not in {"http", "https"} or not urlparse(proxy).hostname):
        raise ValueError("DISCORD_PROXY 需要 HTTP/HTTPS 代理地址。")
    flag = os.getenv("ALLOW_DISCORD_IMPORT", "false").lower()
    if flag not in {"true", "false"}:
        raise ValueError("ALLOW_DISCORD_IMPORT 只能为 true 或 false。")
    data = Path(os.getenv("DATA_DIR", "./data"))
    return Settings(token, allowed, key, data if data.is_absolute() else ROOT / data,
                    guild, proxy, flag == "true")

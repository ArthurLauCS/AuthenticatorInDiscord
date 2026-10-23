import hashlib
import math
import re
import time
from dataclasses import dataclass, field
from urllib.parse import parse_qs, urlparse

import pyotp


@dataclass(frozen=True)
class OTPConfig:
    secret: str = field(repr=False)
    algorithm: str = "SHA1"
    digits: int = 6
    period: int = 30

    def totp(self):
        return pyotp.TOTP(self.secret, digits=self.digits,
                          digest=getattr(hashlib, self.algorithm.lower()), interval=self.period)

    def code(self, now=None):
        now = time.time() if now is None else now
        return self.totp().at(now), math.ceil(self.period - now % self.period)


def parse_secret(value: str) -> OTPConfig:
    value = value.strip()
    if len(value) > 4096:
        raise ValueError("密钥或链接过长。")
    if value.startswith("otpauth-migration:"):
        raise ValueError("Google 批量导出链接请使用本机 import-google 命令导入。")
    if value.startswith("otpauth:"):
        try:
            parsed = urlparse(value)
            params = parse_qs(parsed.query, keep_blank_values=True)
            if parsed.netloc != "totp" or any(len(v) != 1 for v in params.values()):
                raise ValueError
            if "encoder" in params:
                raise ValueError
            otp = pyotp.parse_uri(value)
            if not isinstance(otp, pyotp.TOTP):
                raise ValueError
            result = OTPConfig(otp.secret, otp.digest().name.upper(), otp.digits, otp.interval)
        except Exception:
            # Parser exceptions may contain the submitted seed or URI.
            raise ValueError("链接无效；仅支持标准 otpauth://totp 链接。") from None
    else:
        result = OTPConfig(re.sub(r"\s+", "", value).upper())
    secret = result.secret.upper().rstrip("=")
    if not re.fullmatch(r"[A-Z2-7]{16,1024}", secret):
        raise ValueError("请输入至少 16 位的 Base32 密钥，不是当前的六位验证码。")
    if result.algorithm not in {"SHA1", "SHA256", "SHA512"} or result.digits not in {6, 8}:
        raise ValueError("仅支持 SHA1/SHA256/SHA512 和 6/8 位验证码。")
    if not 15 <= result.period <= 120:
        raise ValueError("验证码周期必须在 15–120 秒之间。")
    result = OTPConfig(secret, result.algorithm, result.digits, result.period)
    try:
        result.totp().byte_secret()
    except Exception:
        raise ValueError("Base32 密钥编码无效。") from None
    return result

import base64
import binascii
from dataclasses import dataclass, field
from urllib.parse import parse_qs, urlencode, urlparse

from google.protobuf.message import DecodeError

from .googleauth_pb2 import MigrationPayload
from .otp import OTPConfig, parse_secret


@dataclass(frozen=True)
class ImportedEntry:
    label: str
    note: str
    otp: OTPConfig = field(repr=False)


def parse_migration(uris: list[str]) -> list[ImportedEntry]:
    if not uris or len(uris) > 100:
        raise ValueError("请提供 1–100 个导出链接。")
    payloads = []
    for uri in uris:
        try:
            if len(uri) > 100_000:
                raise ValueError
            parsed = urlparse(uri.strip())
            query = parse_qs(parsed.query, keep_blank_values=True)
            if parsed.scheme != "otpauth-migration" or parsed.netloc != "offline" or len(query.get("data", [])) != 1:
                raise ValueError
            raw = base64.b64decode(query["data"][0], validate=True)
            payload = MigrationPayload.FromString(raw)
            if payload.version not in (0, 1) or not payload.otp_parameters:
                raise ValueError
        except (ValueError, DecodeError, binascii.Error):
            raise ValueError("Google Authenticator 导出链接无效或版本不支持。") from None
        payloads.append(payload)
    first = payloads[0]
    count = first.batch_size or 1
    if (count != len(payloads) or {p.batch_index for p in payloads} != set(range(count))
            or any((p.batch_size or 1) != count or p.batch_id != first.batch_id for p in payloads)):
        raise ValueError("导出批次不完整、重复或混合了不同批次，请一次提供同一批次的全部二维码链接。")
    result = []
    for payload in sorted(payloads, key=lambda p: p.batch_index):
        for item in payload.otp_parameters:
            if item.type != MigrationPayload.OTP_TYPE_TOTP:
                raise ValueError("导出包含 HOTP 或未知类型；请仅选择 TOTP 账号重新导出，未写入任何条目。")
            algorithm = {1: "SHA1", 2: "SHA256", 3: "SHA512"}.get(item.algorithm)
            digits = {1: 6, 2: 8}.get(item.digits)
            if not algorithm or not digits:
                raise ValueError("导出包含不支持的算法或位数，未写入任何条目。")
            secret = base64.b32encode(item.secret).decode().rstrip("=")
            otp = parse_secret("otpauth://totp/import?" + urlencode({
                "secret": secret, "algorithm": algorithm, "digits": digits, "period": 30}))
            label = " · ".join(x for x in (item.issuer, item.name) if x).strip() or "未命名账号"
            result.append(ImportedEntry(label[:80], item.issuer[:300], otp))
    return result

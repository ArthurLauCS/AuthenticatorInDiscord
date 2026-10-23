import json
import os
import sqlite3
import uuid
from contextlib import closing
from dataclasses import asdict, dataclass, field
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

from .otp import OTPConfig


@dataclass(frozen=True)
class Entry:
    id: str
    label: str
    note: str
    otp: OTPConfig = field(repr=False)


def validate_metadata(label, note):
    label, note = label.strip(), note.strip()
    if not 1 <= len(label) <= 80 or len(note) > 300:
        raise ValueError("名称需为 1–80 个字符，备注最多 300 个字符。")
    return label, note


class Store:
    def __init__(self, path: Path, key: str):
        self.path = path
        self.fernet = Fernet(key.encode("ascii"))
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        with closing(self.connect()) as db, db:
            db.execute("CREATE TABLE IF NOT EXISTS meta (id INTEGER PRIMARY KEY CHECK(id=1), token BLOB NOT NULL)")
            db.execute("CREATE TABLE IF NOT EXISTS entries (id TEXT PRIMARY KEY, owner TEXT NOT NULL, payload BLOB NOT NULL)")
            db.execute("CREATE INDEX IF NOT EXISTS entries_owner ON entries(owner)")
            db.execute("INSERT OR IGNORE INTO meta VALUES (1, ?)",
                       (self.fernet.encrypt(b"authbot-v1"),))
            try:
                if self.fernet.decrypt(db.execute("SELECT token FROM meta WHERE id=1").fetchone()[0]) != b"authbot-v1":
                    raise InvalidToken
            except InvalidToken:
                raise ValueError("数据库无法解密，请恢复原 ENCRYPTION_KEY；不要重建或覆盖数据。") from None
        if os.name != "nt":
            path.chmod(0o600)

    def connect(self):
        return sqlite3.connect(self.path, timeout=10)

    def _encode(self, entry_id, owner, label, note, otp):
        # Bind ciphertext to its row and owner to prevent ciphertext swapping.
        return self.fernet.encrypt(json.dumps({"id": entry_id, "owner": str(owner),
            "label": label, "note": note, "otp": asdict(otp)}, ensure_ascii=False).encode())

    def _decode(self, row, owner):
        try:
            data = json.loads(self.fernet.decrypt(row[1]))
            if data["id"] != row[0] or data["owner"] != str(owner):
                raise ValueError
            return Entry(row[0], data["label"], data["note"], OTPConfig(**data["otp"]))
        except (InvalidToken, ValueError, KeyError, TypeError):
            raise ValueError("条目无法解密或已损坏，请检查密钥或恢复备份。") from None

    def add(self, owner, label, note, otp):
        label, note = validate_metadata(label, note)
        entry_id = uuid.uuid4().hex
        with closing(self.connect()) as db, db:
            db.execute("INSERT INTO entries VALUES (?, ?, ?)",
                       (entry_id, str(owner), self._encode(entry_id, owner, label, note, otp)))
        return entry_id

    def list(self, owner):
        with closing(self.connect()) as db:
            rows = db.execute("SELECT id, payload FROM entries WHERE owner=? ORDER BY rowid", (str(owner),)).fetchall()
        return [self._decode(row, owner) for row in rows]

    def get(self, owner, entry_id):
        with closing(self.connect()) as db:
            row = db.execute("SELECT id, payload FROM entries WHERE id=? AND owner=?", (entry_id, str(owner))).fetchone()
        if row is None:
            raise ValueError("条目不存在或无权访问，请重新打开 /auth。")
        return self._decode(row, owner)

    def edit(self, owner, entry_id, label, note):
        label, note = validate_metadata(label, note)
        entry = self.get(owner, entry_id)
        with closing(self.connect()) as db, db:
            changed = db.execute("UPDATE entries SET payload=? WHERE id=? AND owner=?",
                (self._encode(entry_id, owner, label, note, entry.otp), entry_id, str(owner))).rowcount
            if not changed:
                raise ValueError("条目已被删除，请重新打开 /auth。")

    def delete(self, owner, entry_id):
        with closing(self.connect()) as db, db:
            if not db.execute("DELETE FROM entries WHERE id=? AND owner=?", (entry_id, str(owner))).rowcount:
                raise ValueError("条目不存在或无权访问。")

    def import_many(self, owner, entries):
        added = 0
        with closing(self.connect()) as db, db:
            db.execute("BEGIN IMMEDIATE")
            rows = db.execute("SELECT id, payload FROM entries WHERE owner=?", (str(owner),)).fetchall()
            known = {self._decode(row, owner).otp for row in rows}
            for entry in entries:
                label, note = validate_metadata(entry.label, entry.note)
                if entry.otp in known:
                    continue
                entry_id = uuid.uuid4().hex
                db.execute("INSERT INTO entries VALUES (?, ?, ?)",
                    (entry_id, str(owner), self._encode(entry_id, owner, label, note, entry.otp)))
                known.add(entry.otp)
                added += 1
        return added, len(entries) - added

    def backup(self, destination: Path):
        destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        # Exclusive creation prevents silently replacing an existing backup.
        with destination.open("xb"):
            pass
        if os.name != "nt":
            destination.chmod(0o600)
        with closing(self.connect()) as source, closing(sqlite3.connect(destination)) as target:
            source.backup(target)

import base64
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path
from urllib.parse import urlencode

from cryptography.fernet import Fernet

from authbot.googleauth_pb2 import MigrationPayload
from authbot.migration import ImportedEntry, parse_migration
from authbot.otp import OTPConfig, parse_secret
from authbot.store import Store

SECRET = "JBSWY3DPEHPK3PXP"


def migration_uri(index=0, size=1, batch=7, algorithm=1, kind=2):
    payload = MigrationPayload(version=1, batch_size=size, batch_index=index, batch_id=batch)
    payload.otp_parameters.add(secret=b"12345678901234567890", name="test@example.com",
                               issuer="Example", algorithm=algorithm, digits=1, type=kind)
    return "otpauth-migration://offline?" + urlencode({"data": base64.b64encode(payload.SerializeToString()).decode()})


class OTPTests(unittest.TestCase):
    def test_rfc6238_vectors_and_expiry_boundary(self):
        for algorithm, raw, expected in [
            ("SHA1", b"12345678901234567890", "94287082"),
            ("SHA256", b"12345678901234567890123456789012", "46119246"),
            ("SHA512", b"1234567890123456789012345678901234567890123456789012345678901234", "90693936"),
        ]:
            with self.subTest(algorithm=algorithm):
                otp = OTPConfig(base64.b32encode(raw).decode(), algorithm, 8, 30)
                self.assertEqual(otp.code(59), (expected, 1))
                self.assertEqual(otp.code(60)[1], 30)
                self.assertNotEqual(otp.code(60)[0], expected)

    def test_secret_and_uri(self):
        self.assertEqual(parse_secret(" jbsw y3dp ehpk 3pxp ").secret, SECRET)
        uri = f"otpauth://totp/Account?secret={SECRET}&algorithm=SHA256&digits=8&period=60"
        self.assertEqual(parse_secret(uri), OTPConfig(SECRET, "SHA256", 8, 60))
        self.assertNotIn(SECRET, repr(parse_secret(SECRET)))

    def test_reject_invalid_without_echoing_secrets(self):
        for value in ["123456", "invalid-secret", f"otpauth://hotp/test?secret={SECRET}&counter=1",
                      f"otpauth://totp/test?secret={SECRET}&digits=9",
                      f"otpauth://totp/test?secret={SECRET}&secret={SECRET}",
                      f"otpauth://totp/test?secret={SECRET}&period=0",
                      "otpauth-migration://offline?data=secret"]:
            with self.subTest(value=value), self.assertRaises(ValueError) as error:
                parse_secret(value)
            self.assertNotIn(SECRET, str(error.exception))


class MigrationTests(unittest.TestCase):
    def test_import_algorithm_metadata_and_batch_order(self):
        result = parse_migration([migration_uri(1, 2, algorithm=2), migration_uri(0, 2)])
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0].label, "Example · test@example.com")
        self.assertEqual(result[1].otp.algorithm, "SHA256")
        self.assertEqual(result[0].otp.code(59)[0], "287082")

    def test_incomplete_duplicate_mixed_and_unsupported_batches(self):
        cases = [[migration_uri(0, 2)], [migration_uri(0, 2)] * 2,
                 [migration_uri(0, 2), migration_uri(1, 2, batch=8)],
                 [migration_uri(kind=1)], [migration_uri(algorithm=4)],
                 ["otpauth-migration://offline?data=bad"], []]
        for uris in cases:
            with self.subTest(uris=uris), self.assertRaises(ValueError):
                parse_migration(uris)


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "vault.db"
        self.key = Fernet.generate_key().decode()
        self.store = Store(self.path, self.key)
        self.otp = parse_secret(SECRET)

    def test_encryption_restart_and_wrong_key(self):
        item = self.store.add(1, "PrivateLabel", "PrivateNote", self.otp)
        for secret in (SECRET, "PrivateLabel", "PrivateNote"):
            self.assertNotIn(secret.encode(), self.path.read_bytes())
        reopened = Store(self.path, self.key)
        self.assertEqual(reopened.get(1, item).otp, self.otp)
        with self.assertRaises(ValueError):
            Store(self.path, Fernet.generate_key().decode())
        self.assertEqual(reopened.get(1, item).label, "PrivateLabel")

    def test_user_isolation_edit_delete_and_stale_ids(self):
        item = self.store.add(1, "Account", "original", self.otp)
        self.assertEqual(self.store.list(2), [])
        for action in (lambda: self.store.get(2, item),
                       lambda: self.store.edit(2, item, "other", "other"),
                       lambda: self.store.delete(2, item)):
            with self.assertRaises(ValueError):
                action()
        self.store.edit(1, item, "renamed", "new note")
        self.assertEqual(self.store.get(1, item).note, "new note")
        self.store.delete(1, item)
        with self.assertRaises(ValueError):
            self.store.get(1, item)

    def test_import_atomicity_dedup_and_backup(self):
        entry = ImportedEntry("name", "note", self.otp)
        with self.assertRaises(ValueError):
            self.store.import_many(1, [entry, ImportedEntry("", "", OTPConfig("A" * 32))])
        self.assertEqual(self.store.list(1), [])
        self.assertEqual(self.store.import_many(1, [entry, entry]), (1, 1))
        self.assertEqual(self.store.import_many(1, [entry]), (0, 1))
        target = self.path.with_name("backup.db")
        self.store.backup(target)
        self.assertEqual(Store(target, self.key).list(1), self.store.list(1))
        with self.assertRaises(FileExistsError):
            self.store.backup(target)

    def test_ciphertext_cannot_be_swapped_between_owners(self):
        a = self.store.add(1, "a", "", self.otp)
        b = self.store.add(2, "b", "", self.otp)
        with closing(sqlite3.connect(self.path)) as db, db:
            db.execute("UPDATE entries SET payload=(SELECT payload FROM entries WHERE id=?) WHERE id=?", (a, b))
        with self.assertRaises(ValueError):
            self.store.get(2, b)

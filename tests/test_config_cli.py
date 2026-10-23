import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cryptography.fernet import Fernet
from dotenv import dotenv_values

from authbot.__main__ import main, setup
from authbot.config import load_settings
from authbot.store import Store
from test_core import migration_uri


class ConfigCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.env = {"ALLOWED_USER_IDS": "1,2", "ENCRYPTION_KEY": Fernet.generate_key().decode(),
                    "DATA_DIR": str(self.path / "data")}

    def test_config_fails_closed(self):
        with patch.dict(os.environ, self.env, clear=True), patch("authbot.config.load_dotenv"):
            self.assertEqual(load_settings().allowed_users, frozenset({1, 2}))
            with self.assertRaises(ValueError):
                load_settings(require_token=True)
            for key, value in [("ALLOWED_USER_IDS", ""), ("ENCRYPTION_KEY", "bad"),
                               ("ALLOWED_USER_IDS", "0"), ("ALLOW_DISCORD_IMPORT", "yes"),
                               ("DISCORD_PROXY", "socks5://localhost:7890")]:
                with self.subTest(key=key), patch.dict(os.environ, {key: value}), self.assertRaises(ValueError):
                    load_settings()

    def test_help_works_with_windows_legacy_pipe_encoding(self):
        result = subprocess.run([sys.executable, "-m", "authbot", "--help"],
            env={**os.environ, "PYTHONIOENCODING": "cp1252"}, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8"))
        self.assertIn("验证器", result.stdout.decode("utf-8"))

    def test_setup_preserves_existing_key_and_hides_token(self):
        output = io.StringIO()
        with patch("authbot.__main__.ROOT", self.path), patch("authbot.__main__.hidden_input", return_value="dummy-token"), \
             patch("builtins.input", return_value="1"), contextlib.redirect_stdout(output):
            setup()
            original = (self.path / ".env").read_bytes()
            with self.assertRaises(ValueError):
                setup()
            self.assertEqual((self.path / ".env").read_bytes(), original)
        values = dotenv_values(self.path / ".env")
        Fernet(values["ENCRYPTION_KEY"].encode())
        self.assertNotIn("dummy-token", output.getvalue())
        self.assertNotIn(values["ENCRYPTION_KEY"], output.getvalue())

    def test_google_cli_to_encrypted_vault(self):
        output = io.StringIO()
        with patch.dict(os.environ, self.env, clear=True), patch("authbot.config.load_dotenv"), \
             patch("sys.argv", ["authbot", "import-google", "--user", "1"]), \
             patch("authbot.__main__.hidden_input", side_effect=[migration_uri(), ""]), \
             contextlib.redirect_stdout(output):
            self.assertEqual(main(), 0)
        store = Store(self.path / "data" / "vault.db", self.env["ENCRYPTION_KEY"])
        entries = store.list(1)
        self.assertEqual(len(entries), 1)
        self.assertNotIn(entries[0].otp.secret, output.getvalue())
        self.assertEqual(store.list(2), [])

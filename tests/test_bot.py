import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock, patch

from cryptography.fernet import Fernet

from authbot.bot import AuthBot, DeleteView, EntryButton, EntryModal, EntryView, PanelView, authorized
from authbot.config import Settings
from authbot.otp import parse_secret
from authbot.store import Store


def interaction(user=1):
    return SimpleNamespace(user=SimpleNamespace(id=user),
        response=SimpleNamespace(is_done=Mock(return_value=False), send_message=AsyncMock(),
            send_modal=AsyncMock(), defer=AsyncMock(), edit_message=AsyncMock()),
        followup=SimpleNamespace(send=AsyncMock()), edit_original_response=AsyncMock(), message=None)


class BotTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        key = Fernet.generate_key().decode()
        self.store = Store(Path(self.temp.name) / "vault.db", key)
        self.settings = Settings("fake-token", frozenset({1, 2}), key, Path(self.temp.name))
        self.bot = AuthBot(self.settings, self.store)
        self.addAsyncCleanup(self.bot.close)
        self.otp = parse_secret("JBSWY3DPEHPK3PXP")
        self.item = self.store.add(1, "Account", "备注", self.otp)

    async def test_authorization_for_command_view_modal_and_confirmation(self):
        self.assertFalse(await self.bot.tree.interaction_check(interaction(999)))
        self.assertTrue(await authorized(interaction(1), self.settings))
        entry = self.store.get(1, self.item)
        for surface in (PanelView(self.bot, 1, [entry], 0), EntryView(self.bot, 1, entry),
                        EntryModal(self.bot, 1, entry), DeleteView(self.bot, 1, self.item)):
            denied = interaction(2)
            self.assertFalse(await surface.interaction_check(denied))
            self.assertTrue(denied.response.send_message.call_args.kwargs["ephemeral"])

    async def test_click_reads_fresh_entry_and_returns_private_code(self):
        panel = PanelView(self.bot, 1, self.store.list(1), 0)
        button = next(x for x in panel.children if isinstance(x, EntryButton))
        self.store.edit(1, self.item, "renamed", "new note")
        event = interaction()
        with patch("authbot.bot.time.time", return_value=60), patch.object(self.bot, "expire_message") as expire:
            await button.callback(event)
        sent = event.followup.send.call_args.kwargs
        self.assertTrue(sent["ephemeral"])
        self.assertEqual(sent["embed"].title, "renamed")
        self.assertIn(self.otp.code(60)[0], sent["embed"].description)
        self.assertNotIn(self.otp.secret, str(sent["embed"].to_dict()))
        expire.assert_called_once()
        self.store.delete(1, self.item)
        with self.assertRaises(ValueError):
            await button.callback(interaction())

    async def test_pagination_empty_state_and_discord_limits(self):
        for _ in range(24):
            self.store.add(1, "*" * 80, "*" * 300, self.otp)
        for page, expected in [(0, 10), (1, 10), (2, 5), (999, 5)]:
            event = interaction()
            await self.bot.send_panel(event, page)
            kwargs = event.followup.send.call_args.kwargs
            self.assertLessEqual(len(kwargs["embed"]), 6000)
            self.assertEqual(len(kwargs["embed"].fields), expected)
            self.assertEqual(len(kwargs["view"].to_components()), 3 if expected > 5 else 2)
        event = interaction(2)
        await self.bot.send_panel(event)
        self.assertEqual(len(event.followup.send.call_args.kwargs["embed"].fields), 0)

    async def test_import_disabled_and_delete_requires_confirmation(self):
        event = interaction()
        panel = PanelView(self.bot, 1, self.store.list(1), 0)
        await panel.add.callback(event)
        event.response.send_modal.assert_not_called()
        entry_view = EntryView(self.bot, 1, self.store.get(1, self.item))
        await entry_view.delete.callback(event)
        self.assertEqual(len(self.store.list(1)), 1)
        confirmation = event.response.send_message.call_args.kwargs["view"]
        await confirmation.confirm.callback(interaction())
        self.assertEqual(self.store.list(1), [])

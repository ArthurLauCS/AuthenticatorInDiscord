import asyncio
import logging
import math
import time

import discord
from discord import app_commands

from .otp import parse_secret

log = logging.getLogger("authbot")
PAGE_SIZE = 10


def safe_text(text):
    return discord.utils.escape_mentions(discord.utils.escape_markdown(text))


async def private_reply(interaction, content):
    if interaction.response.is_done():
        await interaction.followup.send(content, ephemeral=True)
    else:
        await interaction.response.send_message(content, ephemeral=True)


async def report_error(interaction, error):
    # Never log exception text/tracebacks: parser or HTTP errors can carry secrets.
    log.warning("Interaction failed (%s)", type(error).__name__)
    message = str(error) if isinstance(error, ValueError) else "操作失败，请稍后重试；若持续失败，请检查服务配置和数据库。"
    try:
        await private_reply(interaction, message)
    except discord.HTTPException:
        pass


async def authorized(interaction, settings, owner=None):
    if interaction.user.id not in settings.allowed_users or (owner is not None and interaction.user.id != owner):
        await private_reply(interaction, "无权使用此验证器。")
        return False
    return True


class PrivateView(discord.ui.View):
    def __init__(self, bot, owner, timeout=600):
        super().__init__(timeout=timeout)
        self.bot, self.owner = bot, owner

    async def interaction_check(self, interaction):
        return await authorized(interaction, self.bot.settings, self.owner)

    async def on_error(self, interaction, error, item):
        await report_error(interaction, error)


class EntryModal(discord.ui.Modal):
    def __init__(self, bot, owner, entry=None):
        super().__init__(title="修改名称与备注" if entry else "新增验证器（密钥将经由 Discord）", timeout=300)
        self.bot, self.owner, self.entry = bot, owner, entry
        self.label_input = discord.ui.TextInput(label="名称", max_length=80, default=entry.label if entry else None)
        self.note_input = discord.ui.TextInput(label="备注", style=discord.TextStyle.paragraph,
            required=False, max_length=300, default=entry.note if entry else None)
        self.add_item(self.label_input)
        self.add_item(self.note_input)
        if entry is None:
            self.secret_input = discord.ui.TextInput(label="Base32 密钥或 otpauth://totp 链接",
                style=discord.TextStyle.paragraph, max_length=4000)
            self.add_item(self.secret_input)

    async def interaction_check(self, interaction):
        return await authorized(interaction, self.bot.settings, self.owner)

    async def on_submit(self, interaction):
        await interaction.response.defer(ephemeral=True, thinking=True)
        if self.entry is None:
            if not self.bot.settings.discord_import:
                raise ValueError("Discord 导入已关闭，请使用本机 add 或 import-google 命令。")
            otp = parse_secret(str(self.secret_input))
            await asyncio.to_thread(self.bot.store.add, self.owner, str(self.label_input), str(self.note_input), otp)
        else:
            await asyncio.to_thread(self.bot.store.edit, self.owner, self.entry.id, str(self.label_input), str(self.note_input))
        await self.bot.send_panel(interaction)

    async def on_error(self, interaction, error):
        await report_error(interaction, error)


class DeleteView(PrivateView):
    def __init__(self, bot, owner, entry_id):
        super().__init__(bot, owner, timeout=60)
        self.entry_id = entry_id

    @discord.ui.button(label="确认删除", style=discord.ButtonStyle.danger)
    async def confirm(self, interaction, button):
        await interaction.response.defer(ephemeral=True, thinking=True)
        await asyncio.to_thread(self.bot.store.delete, self.owner, self.entry_id)
        self.stop()
        if interaction.message:
            await interaction.message.edit(content="条目已删除。", view=None)
        await self.bot.send_panel(interaction)

    @discord.ui.button(label="取消")
    async def cancel(self, interaction, button):
        self.stop()
        await interaction.response.edit_message(content="已取消。", view=None)


class EntryView(PrivateView):
    def __init__(self, bot, owner, entry):
        super().__init__(bot, owner, timeout=120)
        self.entry = entry

    @discord.ui.button(label="修改名称 / 备注")
    async def edit(self, interaction, button):
        await interaction.response.send_modal(EntryModal(self.bot, self.owner, self.entry))

    @discord.ui.button(label="删除条目", style=discord.ButtonStyle.danger)
    async def delete(self, interaction, button):
        await interaction.response.send_message("确定删除这个条目？删除后需重新导入密钥。",
            ephemeral=True, view=DeleteView(self.bot, self.owner, self.entry.id))


class EntryButton(discord.ui.Button):
    def __init__(self, entry, index):
        super().__init__(label=f"{index + 1}. {entry.label}"[:80], style=discord.ButtonStyle.primary, row=index // 5)
        self.entry_id = entry.id

    async def callback(self, interaction):
        await interaction.response.defer(ephemeral=True, thinking=True)
        view = self.view
        # Re-read with owner filtering on every click, including stale panels.
        entry = await asyncio.to_thread(view.bot.store.get, view.owner, self.entry_id)
        code, remaining = entry.otp.code()
        if remaining <= 3:
            await asyncio.sleep(remaining + 0.1)
            code, remaining = entry.otp.code()
        expires = (math.floor(time.time() / entry.otp.period) + 1) * entry.otp.period
        embed = discord.Embed(title=safe_text(entry.label), description=f"### `{code}`", color=0x5865F2)
        embed.add_field(name="备注", value=safe_text(entry.note) or "未填写", inline=False)
        embed.add_field(name="有效期", value=f"<t:{expires}:T> 到期（<t:{expires}:R>）", inline=False)
        embed.set_footer(text="需要新验证码时，再次点击面板中的条目。")
        message = await interaction.followup.send(embed=embed, ephemeral=True,
            view=EntryView(view.bot, view.owner, entry), wait=True)
        view.bot.expire_message(message, remaining)


class PanelView(PrivateView):
    def __init__(self, bot, owner, entries, page):
        super().__init__(bot, owner)
        self.page = page
        for index, entry in enumerate(entries[page * PAGE_SIZE:(page + 1) * PAGE_SIZE]):
            self.add_item(EntryButton(entry, index))
        self.previous.disabled = page == 0
        self.next_page.disabled = (page + 1) * PAGE_SIZE >= len(entries)

    @discord.ui.button(label="上一页", row=2)
    async def previous(self, interaction, button):
        await interaction.response.defer()
        await self.bot.send_panel(interaction, self.page - 1, edit=True)

    @discord.ui.button(label="下一页", row=2)
    async def next_page(self, interaction, button):
        await interaction.response.defer()
        await self.bot.send_panel(interaction, self.page + 1, edit=True)

    @discord.ui.button(label="刷新列表", row=2)
    async def refresh(self, interaction, button):
        await interaction.response.defer()
        await self.bot.send_panel(interaction, self.page, edit=True)

    @discord.ui.button(label="新增条目", style=discord.ButtonStyle.success, row=2)
    async def add(self, interaction, button):
        if self.bot.settings.discord_import:
            await interaction.response.send_modal(EntryModal(self.bot, self.owner))
        else:
            await private_reply(interaction,
                "请在服务所在电脑运行 `python -m authbot add`，或用 `python -m authbot import-google` 导入 Google 导出链接。\n"
                "如需在 Discord 表单中输入密钥，在 .env 设置 ALLOW_DISCORD_IMPORT=true 并重启服务。")


class AuthTree(app_commands.CommandTree):
    async def interaction_check(self, interaction):
        return await authorized(interaction, self.client.settings)

    async def on_error(self, interaction, error):
        await report_error(interaction, getattr(error, "original", error))


class AuthBot(discord.Client):
    def __init__(self, settings, store):
        super().__init__(intents=discord.Intents.none(), proxy=settings.proxy,
                         allowed_mentions=discord.AllowedMentions.none())
        self.settings, self.store = settings, store
        self.tree = AuthTree(self)
        self.expiry_tasks = set()

        @self.tree.command(name="auth", description="打开我的验证器：查看备注、点击获取验证码")
        async def auth(interaction: discord.Interaction):
            await interaction.response.defer(ephemeral=True, thinking=True)
            await self.send_panel(interaction)

    async def setup_hook(self):
        if self.settings.guild_id:
            guild = discord.Object(id=self.settings.guild_id)
            self.tree.copy_global_to(guild=guild)
            await self.tree.sync(guild=guild)
        else:
            await self.tree.sync()

    async def on_ready(self):
        log.info("Bot connected; /auth is ready.")

    async def on_error(self, event_method, *args, **kwargs):
        log.warning("Discord event failed (%s)", event_method)

    async def send_panel(self, interaction, page=0, edit=False):
        entries = await asyncio.to_thread(self.store.list, interaction.user.id)
        pages = max(1, math.ceil(len(entries) / PAGE_SIZE))
        page = min(max(0, page), pages - 1)
        embed = discord.Embed(title="我的验证器", color=0x5865F2,
            description="点击下方对应按钮获取验证码。" if entries else "还没有条目，点击「新增条目」开始。")
        for index, entry in enumerate(entries[page * PAGE_SIZE:(page + 1) * PAGE_SIZE]):
            embed.add_field(name=f"{index + 1}. {safe_text(entry.label)}",
                            value=safe_text(entry.note)[:350] or "未填写备注", inline=False)
        embed.set_footer(text=f"第 {page + 1}/{pages} 页 · 共 {len(entries)} 条 · 面板 10 分钟有效，失效后重新输入 /auth")
        view = PanelView(self, interaction.user.id, entries, page)
        if edit:
            await interaction.edit_original_response(embed=embed, view=view)
        else:
            await interaction.followup.send(embed=embed, view=view, ephemeral=True)

    def expire_message(self, message, delay):
        async def remove():
            await asyncio.sleep(delay)
            try:
                await message.delete()
            except discord.HTTPException:
                pass
        task = asyncio.create_task(remove())
        self.expiry_tasks.add(task)
        task.add_done_callback(self.expiry_tasks.discard)

    async def close(self):
        for task in self.expiry_tasks:
            task.cancel()
        await super().close()

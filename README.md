# AuthTile

English | [简体中文](README.zh-CN.md)

A Windows desktop tile that shows your Google Authenticator codes, live. Import the QR code that Google Authenticator exports, and every account appears as a row with its current code and a countdown ring.

- A ~1.3 MB exe (the installer is about 2.7 MB, with the bundled font). About 6.5 MB of private memory; around 40 MB once Windows loads your input method into it.
- Follows the system light / dark theme; adjustable opacity, down to a see-through tile.
- English, Simplified Chinese and Traditional Chinese interface.
- Swiss-style type-led look: square, ruled rows, the codes set large in TeX Gyre Heros Bold; Chinese text uses Microsoft YaHei UI (Simplified) or Microsoft JhengHei UI (Traditional).

## Install

Download `AuthTile-Setup-<version>.exe` from [Releases](https://github.com/ArthurLauCS/AuthenticatorInDiscord/releases/latest) and run it. Requires 64-bit Windows 10 or 11.

- The installer is not signed with a commercial code-signing certificate, so Windows may show "Windows protected your PC". Click **More info** → **Run anyway**.
- It installs for the current user only and does not ask for administrator permission.
- It carries one open-source font, TeX Gyre Heros (GUST Font License, see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)), in a `fonts` folder next to the exe. If that folder is missing, the tile falls back to Arial.
- The language you pick in setup becomes the interface language. You can change it later from the menu.

## Importing accounts

In Google Authenticator, use **Transfer accounts → Export accounts** and capture the QR code it shows. If your phone blocks screenshots of that screen, take a photo of it with another device.

Then give the image to the tile in any of these ways:

- Drag one or more image files onto the tile.
- Click **+** in the tile's header and pick the images.
- Copy the image, or the image files in Explorer, and press **Ctrl+V** on the tile (or choose **Import from clipboard** in the menu).

PNG, JPG, WebP and BMP work, and one image may hold several QR codes. Standard `otpauth://totp/...` QR codes and copied `otpauth://` links are accepted too. Accounts that are already on the tile are skipped, and HOTP (counter-based) accounts are not supported; the tile tells you how many were imported, skipped and unsupported.

## Using the tile

- **Click a row** to copy its code. Copied codes are kept out of the Windows clipboard history (Win+V).
- The ring next to each code shows the time left and turns red in the last 5 seconds.
- **Right-click a row** to copy, rename (emoji welcome), move it up or down, hide it, set a game login, or delete it. Delete asks you to click a second time.
- **Drag a row** to reorder the codes.
- Hidden codes are not deleted: they fold under the **Show N hidden** button at the bottom. Click it to show them, dimmed, below the button; they can still be copied, dragged and unhidden there. The tile starts with them folded.
- **Right-click elsewhere on the tile, or the tray icon,** for settings: opacity, always on top, start with Windows, language, import, hide the tile, exit.
- Drag the tile's header to move it. The pin button in the header toggles always on top. Left-click the tray icon to hide or show the tile.

## One-click sign-in for Lost Ark (Taiwan)

For the Taiwan server of Lost Ark (the MangoT5 launcher), a row can sign you in and start the game: the tile fills in the launcher's account, password and OTP code, then clicks the start button.

1. Right-click the row whose code protects the game account and choose **Set game login…**. Type the account ID and press Enter, then the password and press Enter.
2. Right-click the row again and turn on **Login button**. A gamepad button appears between the code and the ring.
3. Click the button. If the launcher isn't open, the tile starts it. It fills in the account and password, signs in, enters the code on the OTP page and confirms, waits for the launcher's version check, clicks the start button and brings the launcher to the front.

- Only rows you set up get the button; your other codes stay as they are.
- If the launcher is already signed in, or is waiting on its OTP page, the tile leaves it alone and asks you to sign out first, so it never continues someone else's sign-in.
- A wrong password or code shows the launcher's own error message on the tile.
- If the current code has less than 3 seconds left, the tile waits for the next one.
- If the game needs an update, the tile ticks the launcher's own auto-start box and clicks Update, so the launcher starts the game once the update is done. If the launcher asks for a repair, reports an error or shows maintenance, the tile only tells you and leaves it to you.
- To change the password, choose **Set game login…** again; pressing Enter in an empty password box keeps the old one. To remove the login, clear the account ID and press Enter.
- The tile only works inside the launcher's page; it never touches the game itself.

## Your data

The accounts are stored in `%APPDATA%\AuthTile\vault.dat`, encrypted with Windows DPAPI for your Windows user. Only the same Windows user on the same PC can open it, so a copied `vault.dat` is useless elsewhere; on a new PC, import the QR codes again. Keep the exported QR images somewhere safe, or keep the accounts in Google Authenticator as well.

Anyone who can use your unlocked Windows account can see the codes, just as with the phone app when it's unlocked.

Game logins (account ID and password) live in the same file with the same encryption. Since the password and the OTP secret then sit side by side, anyone who can use your unlocked Windows account can also sign in to those game accounts: on this PC the second factor no longer protects them. A password leaked elsewhere is still useless without the code.

Uninstalling asks whether to delete the settings and accounts too; the default keeps them.

## Build from source

Needs Rust (stable, MSVC toolchain).

```powershell
powershell -File scripts\fonts.ps1       # once: downloads the font into tile\assets\fonts (not in git)
cargo test --workspace                  # TOTP, QR decoding, import / dedup, settings
cargo build --release -p authtile       # target\release\authtile.exe
```

- `core` (crate `authcore`) is the engine with no UI: TOTP per RFC 6238 (SHA-1 / SHA-256 / SHA-512, 6 or 8 digits), QR decoding, the `otpauth://` and `otpauth-migration://` parsers, and the account list with import dedup.
- `tile` is the Windows app: Direct2D drawn onto a DirectComposition swap chain with a translucent background. All colors, sizes and font names live in `tile\src\theme.rs`; `tile\src\fontload.rs` loads the bundled font as a DirectWrite font set. The exe looks for fonts in a `fonts` folder next to it, then in `tile\assets\fonts` two levels up (the dev layout). `tile\src\launcher.rs` signs in to the Lost Ark launcher by reading and writing the DOM of the web page in the launcher's embedded Internet Explorer control.
- `scripts\fonts.ps1` fetches TeX Gyre Heros from the GUST e-foundry (pinned URL and SHA-256).
- `scripts\installer.ps1` runs `fonts.ps1`, then builds the installer into `target\installer` (needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)); it keeps only the current version's installer.
- `scripts\icon.py` renders `tile\assets\icon.svg` / `icon-small.svg` into `app.ico` and `app.res` (needs Python with Pillow and Google Chrome).
- `cargo run -p authcore --example mkqr -- out.png "Issuer|name|BASE32SECRET"` makes test QR codes with fake accounts.

The Google Authenticator export format follows `proto/googleauth.proto`; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

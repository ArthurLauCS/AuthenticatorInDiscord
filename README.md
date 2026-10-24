# AuthTile

English | [简体中文](README.zh-CN.md)

A Windows desktop tile that shows your Google Authenticator codes, live. Import the QR code that Google Authenticator exports, and every account appears as a row with its current code and a countdown ring.

- A single ~1.2 MB exe. About 6 MB of private memory; around 40 MB once Windows loads your input method into it.
- Follows the system light / dark theme; adjustable opacity, down to a see-through tile.
- English and Chinese interface.

## Install

Run `AuthTile-Setup-<version>.exe`. Requires 64-bit Windows 10 or 11.

- The installer is not signed with a commercial code-signing certificate, so Windows may show "Windows protected your PC". Click **More info** → **Run anyway**.
- It installs for the current user only and does not ask for administrator permission.
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
- **Right-click a row** to copy, rename (emoji welcome), move it up or down, or delete it. Delete asks you to click a second time.
- **Right-click elsewhere on the tile, or the tray icon,** for settings: opacity, always on top, start with Windows, language, import, hide the tile, exit.
- Drag the tile to move it. The pin button in the header toggles always on top. Left-click the tray icon to hide or show the tile.

## Your data

The accounts are stored in `%APPDATA%\AuthTile\vault.dat`, encrypted with Windows DPAPI for your Windows user. Only the same Windows user on the same PC can open it, so a copied `vault.dat` is useless elsewhere; on a new PC, import the QR codes again. Keep the exported QR images somewhere safe, or keep the accounts in Google Authenticator as well.

Anyone who can use your unlocked Windows account can see the codes, just as with the phone app when it's unlocked.

Uninstalling asks whether to delete the settings and accounts too; the default keeps them.

## Build from source

Needs Rust (stable, MSVC toolchain).

```powershell
cargo test --workspace                  # TOTP, QR decoding, import / dedup, settings
cargo build --release -p authtile       # target\release\authtile.exe
```

- `core` (crate `authcore`) is the engine with no UI: TOTP per RFC 6238 (SHA-1 / SHA-256 / SHA-512, 6 or 8 digits), QR decoding, the `otpauth://` and `otpauth-migration://` parsers, and the account list with import dedup.
- `tile` is the Windows app: Direct2D drawn onto a DirectComposition swap chain, with an acrylic background.
- `scripts\installer.ps1` builds the installer into `target\installer` (needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)); it keeps only the current version's installer.
- `scripts\icon.py` renders `tile\assets\icon.svg` / `icon-small.svg` into `app.ico` and `app.res` (needs Python with Pillow and Google Chrome).
- `cargo run -p authcore --example mkqr -- out.png "Issuer|name|BASE32SECRET"` makes test QR codes with fake accounts.

The Google Authenticator export format follows `proto/googleauth.proto`; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

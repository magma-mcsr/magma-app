# Magma MCSR

Desktop app for [Magma MCSR](https://magmamcsr.com), the stats site for MCSR Ranked, the
competitive Minecraft speedrunning platform.

It shows the Magma website in its own window, and keeps your MCSR Ranked replays in the
background: official replays are only kept for 7 days, so the app downloads them while they
are still available and sends them to Magma for analysis.

## Download

[Download the installer](https://github.com/magma-mcsr/magma-app/releases/latest/download/Magma-MCSR-Setup.exe)
(latest version), or pick one from the [releases](https://github.com/magma-mcsr/magma-app/releases).

- Windows only for now (Windows 10 and 11, 64-bit).
- If your browser says the file "isn't commonly downloaded", choose Keep (in Edge: click "...", then Keep).
- Windows may show "Windows protected your PC" because the installer is not code-signed yet.
  Click "More info", then "Run anyway".
- After that, the app keeps itself up to date.

## What it does

- Downloads replays of the players **you** add, from the official public replay CDN.
- Stays well within the official limits: at most one replay download every 26 seconds, and
  API requests at least 1.2 seconds apart. It identifies itself as `magma-app/<version>`.
- Only downloads replays from the last 7 days that it has not handled before.
- Optionally saves a copy of each replay to a folder you choose.
- Uploads each replay to Magma (`api.magmamcsr.com`) for analysis. **Analyses are public.**
- Checks this repository's GitHub Releases for updates and installs them. An update is only
  installed if it is signed with the Magma update key; release installers are built from this
  repository by GitHub Actions (`.github/workflows/release.yml`).
- If Magma stops accepting an old version of the app, uploads pause (nothing is deleted) until
  the app has updated.

## What it does not do

- It does not read any other files on your computer.
- It does not ask for your Minecraft, Microsoft or MCSR login.
- The website shown in the window cannot use any of the app's native features (files,
  downloads, settings). Only the app's own title bar and settings panel can.

## Where to look

| What | File |
|---|---|
| What gets downloaded, and when | `app/src-tauri/src/sync.rs` |
| What gets sent to Magma | `app/src-tauri/src/upload.rs` |
| Requests to the official API and replay CDN | `app/src-tauri/src/mcsr.rs` |
| Which pages may call native features | `app/src-tauri/capabilities/default.json` |
| Window, title bar and tray | `app/src-tauri/src/shell.rs` |
| Settings and where they are stored | `app/src-tauri/src/settings.rs` |
| Updates | `app/src-tauri/src/update.rs` |

## Building

Requires Node.js 24+ and Rust (stable).

```bash
cd app
npm install
npm run tauri build
```

## AI assistance

This project is developed with the help of AI coding tools (Claude). All changes are
reviewed and tested by a person.

## License

Copyright (C) 2026 Magma MCSR

The source code is released under the [GNU General Public License v3.0](LICENSE)
(GPL-3.0-only). You are free to read, run and modify it; if you distribute a modified
version, it must also be released under the GPL with its source code.

The Magma MCSR name and logo are not covered by the license. Forks must remove them.

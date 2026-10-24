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

- Shows magmamcsr.com in its own window. To the website it looks like Microsoft Edge, with
  `MagmaApp/<version>` added to the user agent so Magma can tell visits from the app apart.
- Links to other websites open in your default browser, and `mailto:` links in your mail app.
  Files you download from the website are saved to your Downloads folder, as in a browser.
- When you close the window to the tray, the website is unloaded and only the background
  sync keeps running.
- Downloads replays of the players **you** add, from the official public replay CDN, in the
  order you list them: if there are more than it can download, the top of your list comes first.
  The first player in your list (usually you) is also checked every 15 seconds for matches that
  ended in the last hour, so a match just played is uploaded within moments; "Check now" in the
  settings does that check right away. These checks count toward the API limit below.
- Stays within the official limits however many players you add: replay downloads start at
  least 21 seconds apart (at most 3 a minute), and API requests at least 1.5 seconds apart (at
  most 400 per 10 minutes, below the official 500). If it is rate limited anyway (for example
  because you watch replays in game on the same connection), it waits as long as asked and
  slows downloads to one every 30 seconds for an hour. It identifies itself as
  `magma-app/<version>`.
- Uses the network connection the system picks, unless you choose one in the settings
  ("Network"). Only the app's own traffic is affected.
- Only downloads replays from the last 7 days that it has not handled before.
- Optionally saves a copy of each replay to a folder you choose (by default your MCSR Ranked
  replay folder, found by looking in the usual launcher folders, so copies show up in game).
- On first run, with no players listed yet, it looks for MCSR Ranked in the usual launcher
  folders (Prism Launcher, MultiMC, Modrinth, CurseForge, the official launcher). MultiMC and
  portable Prism can be unpacked anywhere, so it also looks one level into your home, Desktop,
  Downloads, Documents and Games folders and the drive roots for a folder with an `instances`
  folder and the launcher's program; there it only checks folder names. It then reads the first part of each instance's newest Minecraft logs (`logs/latest.log`,
  then a few older `logs/*.log.gz`), where Minecraft writes "Setting user: <name>" when it
  starts. The names it finds are offered as "Track <name>"; nothing is added until you click.
  The game does not need to be running.
- Uploads each replay to Magma (`api.magmamcsr.com`) for analysis. **Analyses are public.**
- Checks this repository's GitHub Releases for updates. A new version is installed when the app
  starts with Windows, or when you click "Update" in the title bar; it never closes on you
  while you are using it. An update is only installed if it is signed with the Magma update
  key; release installers are built from this repository by GitHub Actions
  (`.github/workflows/release.yml`).
- If Magma stops accepting an old version of the app, uploads pause (nothing is deleted) and the
  app asks you to update or quit.

## Where replays come from

- Replays are downloaded from MCSR Ranked's official public replay server, the same files the
  official site offers. Nothing is taken from anywhere else.
- Magma MCSR is a community project, not affiliated with MCSR Ranked.
- Replay sync depends on MCSR Ranked keeping replays publicly available. If that changes, sync
  may stop working.

## What it does not do

- It does not read any other files on your computer. It never opens launcher account files,
  which hold login tokens.
- It does not ask for your Minecraft, Microsoft or MCSR login.
- The website shown in the window cannot use any of the app's native features (your files,
  settings, replay sync). Only the app's own title bar and settings panel can.
- A link on the website can never open files or programs on your computer: only web links
  and `mailto:` links are passed on to the system.

## Where to look

| What | File |
|---|---|
| What gets downloaded, and when | `app/src-tauri/src/sync.rs` |
| What gets sent to Magma | `app/src-tauri/src/upload.rs` |
| Requests to the official API and replay CDN | `app/src-tauri/src/mcsr.rs` |
| Which pages may call native features | `app/src-tauri/capabilities/default.json` |
| Window, title bar and tray; where links and downloads go | `app/src-tauri/src/shell.rs` |
| Settings and where they are stored | `app/src-tauri/src/settings.rs` |
| Finding MCSR Ranked and the names that played on this computer | `app/src-tauri/src/game.rs` |
| Updates | `app/src-tauri/src/update.rs` |
| What changed in each version | `CHANGELOG.md` |

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

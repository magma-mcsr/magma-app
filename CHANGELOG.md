# Changelog

What changed in each version of the Magma MCSR app. The app shows the section for its own
version after an update, and each GitHub release uses the same text.

## 0.2.3

- A sync statistics report that does not get through is tried again after 10 minutes, instead
  of waiting 6 hours.

## 0.2.2

- **Starts with Windows** for new installs, in the tray, so no match is missed when you forget to
  open the app. The first-run notice says so; turn it off in Settings. Existing installs keep
  their setting.
- Notifications only for matches you play while the app is running. Adding your name, or
  starting the app, no longer pops up a notification for each match of the last hour.
- **Share sync statistics** (Settings, on by default): every 6 hours the app tells Magma it is
  running, with a random ID for this install, the app version, whether sync is on and the players
  in your list. Magma uses it to see how many apps are syncing and which players are covered. Turn
  it off and nothing is sent.

## 0.2.1

- **One app at a time**: after a restart two copies of the app could run, downloading and uploading
  the same replays twice. Opening it again now brings up the one that is already running.
- After an update, What's new shows first instead of hiding behind the Replays tab, and shows each
  item in full.
- While your new match downloads, the title bar says "Downloading your new match" (it said
  "Downloading 1 of 0").

## 0.2.0

- **A Replays tab**: the app opens on its own **Replays** tab, with the website as the second tab,
  like browser tabs. Replays shows your recent matches and whether each one is analyzed yet (click
  one to open it on the website), the players to sync and the sync options. The website stays
  loaded while you are in Replays. App settings and About are behind the gear in the title bar.
- **Notified when your match is analyzed**: a Windows notification when a match you just played
  is analyzed; click it to open the match. Only your own new matches, and it can be turned off
  in Settings.
- **Ready on first launch**: when the app finds your Minecraft name in this computer's Minecraft
  logs (Prism Launcher, MultiMC, Modrinth, CurseForge and the official launcher), it starts
  tracking your matches right away. The Replays tab says so, explains what happens, and lets you
  change the name or turn it off. Without a name found, it asks for yours.
- **Long player lists stay short**: past seven players, the Replays tab shows the first five and
  "Show all" opens the whole list, where you can still reorder and remove players.
- **Help where you need it**: "All settings explained" under the settings, and "Learn more", open
  the app's pages in Magma's docs.
- Matches open on Magma right away from "Open analysis" and from notifications.
- The app uses the same font as the website (Inter).
- The app's pages and update notices use the same slim scrollbar as the website.
- **Faster sync, safely within the limits**
  - Replays download about 25% faster (one every 21 seconds instead of 26), the most the replay
    server allows without blocking.
  - If downloads get blocked anyway, for example while you watch replays in game, the app slows
    down for an hour by itself.
  - Following many players can no longer run into the MCSR Ranked API rate limit.
- **Players at the top first**: when there are more replays than can be downloaded, the players at the
  top of your list always come first, and new matches of top players are found within about
  half an hour even during a big backlog.
- **Your new matches right away**: the first player in your list (usually you) is checked every
  15 seconds, so a match you just finished is analyzed within about a minute instead of waiting for
  the next round. "Check now" in the Replays tab looks immediately.
- **Choose a network**: on a computer with more than one connection (for example wired and
  Wi-Fi), pick which one the app uses under Network in the Replays tab.
- **Watch your replays in game**: when you turn on "Keep a copy of replays", copies now go to
  your MCSR Ranked replay folder by default, so they show up under My Replays. You can still
  pick another folder.

## 0.1.3

- **Smoother What's new**
  - It fades in instead of flashing.
  - The settings panel opens beside it, and back, forward and reload close it first.
- While an update is required, the title bar buttons that act on the website are turned off.
- "Learn more" in the settings now explains what the app does and sends.

## 0.1.2

- **Updates never close the app on you**
  - When a new version is out while you are using the app, the title bar offers "Update to …". It installs when you click it, or the next time the app starts with Windows.
  - If an old version is no longer supported, the app asks you to update or quit instead of closing by itself. Nothing waiting to upload is lost.
- **See what's new**
  - After an update, the app shows what changed. You can open it again from the settings panel.
- **Settings are always there**
  - The panel is now "Settings", with Replay sync and App sections. App settings no longer stay hidden until you add a player.
- Start with Windows is now registered more safely.
- Installers left behind by earlier updates are cleaned up.

## 0.1.1

- **Much lighter in the tray**
  - Closing the window to the tray now unloads the website, so background sync uses a few MB of memory instead of hundreds.
  - Opening it again brings you back to the page you were on, at the same size and position.
- **Links and files behave like in a browser**
  - Links to other websites open in your default browser.
  - Dragging files onto the website now works (for example, worlds in the Run Analyzer).
  - Downloads from the website show their progress in the title bar. Click the finished file to show it in its folder.
- **Fullscreen videos** now cover the whole screen, also when the window is maximized.
- The website can now tell it is opened from the app (`MagmaApp/<version>` in the user agent), for anonymous usage stats.

## 0.1.0

- First release of the Magma MCSR desktop app.
- **The Magma website in its own window**
  - Everything on magmamcsr.com, without opening a browser.
- **Replay sync (early)**
  - Add the MCSR Ranked players you follow.
  - The app downloads their replays in the background while they are still available (official replays are only kept for 7 days).
  - Replays are uploaded to Magma and analyzed.
  - The analyses are not shown on the website yet. They will appear on magmamcsr.com once that part is ready, with no app update needed.
  - Analyses will be public.
  - Optionally keeps a copy of each replay in a folder you choose.

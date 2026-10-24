# Changelog

What changed in each version of the Magma MCSR app. The app shows the section for its own
version after an update, and each GitHub release uses the same text.

## 0.2.0

- **The app has its own tab**: the title bar switches between **Website** and **App**, like browser
  tabs. The App tab fills the window and replaces the settings panel: your recent matches and
  whether each one is analyzed yet (click one to open it on the website), the players to sync,
  and the settings. The website stays loaded while you are in the App tab.
- **Notified when your match is analyzed**: a Windows notification when a match you just played
  is analyzed; click it to open the match. Only your own new matches, and it can be turned off
  in Settings.
- **Ready on first launch**: when the app finds your Minecraft name in this computer's Minecraft
  logs (Prism Launcher, MultiMC, Modrinth, CurseForge and the official launcher), it starts
  tracking your matches right away. The App tab says so, explains what happens, and lets you
  change the name or turn it off. Without a name found, it asks for yours.
- The app uses the same font as the website (Inter).
- The app's pages and update notices use the same slim scrollbar as the website.
- **Faster sync, safely within the limits**
  - Replays download about 25% faster (one every 21 seconds instead of 26), the most the replay
    server allows without blocking.
  - If downloads get blocked anyway, for example while you watch replays in game, the app slows
    down for an hour by itself.
  - Following many players can no longer run into the MCSR Ranked API rate limit.
- **Long player lists**: when there are more replays than can be downloaded, the players at the
  top of your list always come first, and new matches of top players are found within about
  half an hour even during a big backlog.
- **Your new matches right away**: the first player in your list (usually you) is checked every
  15 seconds, so a match you just finished is analyzed within about a minute instead of waiting for
  the next round. "Check now" in the App tab looks immediately.
- **Choose a network**: on a computer with more than one connection (for example wired and
  Wi-Fi), pick which one the app uses in Settings → Network.
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

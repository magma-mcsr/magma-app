// Custom title bar. It runs as its own local webview above the website, so the
// website never needs (or gets) access to native commands.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./titlebar.css";

const run = (action: string) => invoke("titlebar", { action }).catch(console.error);

for (const button of document.querySelectorAll<HTMLButtonElement>("button[data-action]")) {
  button.addEventListener("click", () => run(button.dataset.action!));
}

// Empty areas of the bar move the window; a double click maximizes it.
document.getElementById("bar")!.addEventListener("mousedown", (e) => {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
  run(e.detail === 2 ? "maximize" : "drag");
});

const SECTIONS: Record<string, string> = {
  "": "Home",
  ranked: "Ranked",
  runanalyzer: "Run Analyzer",
  run: "Run Analyzer",
  seed: "Seed Analyzer",
  "seed-history": "Seed Analyzer",
  "seed-replay": "Seed Analyzer",
  events: "Events",
  settings: "Settings",
  about: "About",
  privacy: "Privacy",
  terms: "Terms",
  admin: "Admin",
};

function sectionFor(path: string): string {
  const first = path.split("/")[1] ?? "";
  if (first.startsWith("@")) return "Profile";
  return SECTIONS[first] ?? first.charAt(0).toUpperCase() + first.slice(1);
}

// Sync status pill: it shows the state and opens the settings panel.
type SyncStatus = { state: string; label: string };
const pill = document.getElementById("pill")!;
const showStatus = (s: SyncStatus) => {
  pill.dataset.state = s.state;
  document.getElementById("pill-label")!.textContent = s.label;
};
invoke<SyncStatus>("sync_status").then(showStatus).catch(console.error);
listen<SyncStatus>("sync-status", ({ payload }) => showStatus(payload));
listen<boolean>("panel-open", ({ payload }) => pill.classList.toggle("open", payload));

// A downloaded update shows an "Update" button; clicking it restarts into the new version.
type UpdateStatus = { ready: string | null; updated?: string | null };
const updateButton = document.getElementById("update") as HTMLButtonElement;
const updatedButton = document.getElementById("updated") as HTMLButtonElement;
const showUpdate = (u: UpdateStatus) => {
  updateButton.hidden = !u.ready;
  updateButton.textContent = u.ready ? `Update to ${u.ready}` : "Update";
  updateButton.setAttribute("aria-label", u.ready ? `Restart to update to ${u.ready}` : "Update");
  // Just updated: say so once. Clicking opens the release notes in the browser.
  if (u.updated) {
    updatedButton.textContent = `Updated to ${u.updated}`;
    updatedButton.setAttribute("aria-label", `Updated to ${u.updated}. See what's new`);
    updatedButton.hidden = false;
  }
};
updatedButton.addEventListener("click", () => (updatedButton.hidden = true));

invoke<UpdateStatus>("update_status").then(showUpdate).catch(console.error);
listen<UpdateStatus>("update-status", ({ payload }) => showUpdate(payload));

// A file the website downloads: "Downloading" while it saves, then "Downloaded" for a
// few seconds. Clicking it shows the file in its folder.
const downloadButton = document.getElementById("download") as HTMLButtonElement;
const downloadLabel = document.getElementById("download-label")!;
const downloadName = document.getElementById("download-name")!;
// Most downloads finish instantly; keep "Downloading" up long enough to be seen.
const MIN_BUSY_MS = 900;
// How long the tag takes to fly from the website into this notice (DOWNLOAD_FLY_JS).
const FLY_MS = 550;
let downloadStarted = 0;
let downloadTimer: number | undefined;
listen<string>("download-started", ({ payload }) => {
  window.clearTimeout(downloadTimer);
  downloadStarted = Date.now();
  downloadButton.dataset.state = "busy";
  downloadLabel.textContent = "Downloading";
  downloadName.textContent = payload;
  downloadButton.setAttribute("aria-label", `Downloading ${payload}`);
  downloadButton.hidden = false;
  // Once laid out, send a tag flying here from where the user clicked on the website,
  // and bump this notice when it arrives.
  requestAnimationFrame(() => {
    const rect = downloadButton.getBoundingClientRect();
    invoke("download_fly", { x: rect.left + rect.width / 2, name: payload }).catch(console.error);
    downloadButton.classList.remove("landed");
    window.setTimeout(() => downloadButton.classList.add("landed"), FLY_MS - 60);
  });
});
listen<{ name: string; success: boolean }>("download-finished", ({ payload }) => {
  const wait = Math.max(0, downloadStarted + MIN_BUSY_MS - Date.now());
  window.clearTimeout(downloadTimer);
  downloadTimer = window.setTimeout(() => {
    downloadButton.dataset.state = payload.success ? "done" : "failed";
    downloadLabel.textContent = payload.success ? "Downloaded" : "Download failed";
    downloadName.textContent = payload.name;
    downloadButton.setAttribute("aria-label", payload.success ? `Downloaded ${payload.name}. Show in folder` : `Download failed: ${payload.name}`);
    downloadButton.hidden = false;
    downloadTimer = window.setTimeout(() => (downloadButton.hidden = true), 8000);
  }, wait);
});
downloadButton.addEventListener("click", () => (downloadButton.hidden = true));

// Swap the maximize icon for a restore icon while the window is maximized.
listen<boolean>("window-maximized", ({ payload }) => {
  document.getElementById("maximize")!.classList.toggle("maximized", payload);
});

const section = document.getElementById("section")!;
listen<{ path: string; external: boolean; host: string }>("site-location", ({ payload }) => {
  section.textContent = payload.external ? payload.host : sectionFor(payload.path);
});

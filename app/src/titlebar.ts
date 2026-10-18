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
type UpdateStatus = { ready: string | null };
const updateButton = document.getElementById("update") as HTMLButtonElement;
const showUpdate = (u: UpdateStatus) => {
  updateButton.hidden = !u.ready;
  updateButton.setAttribute("aria-label", u.ready ? `Restart to update to ${u.ready}` : "Update");
};
invoke<UpdateStatus>("update_status").then(showUpdate).catch(console.error);
listen<UpdateStatus>("update-status", ({ payload }) => showUpdate(payload));

// A file the website downloaded: shown for a few seconds, click opens its folder.
const downloadButton = document.getElementById("download") as HTMLButtonElement;
let downloadTimer: number | undefined;
listen<string>("download-finished", ({ payload }) => {
  document.getElementById("download-name")!.textContent = payload;
  downloadButton.setAttribute("aria-label", `Downloaded ${payload}. Show in folder`);
  downloadButton.hidden = false;
  window.clearTimeout(downloadTimer);
  downloadTimer = window.setTimeout(() => (downloadButton.hidden = true), 8000);
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

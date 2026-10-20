// A card over the website, in one of two modes:
// - "required": the server no longer accepts this version. There is deliberately no close
//   button: the only choices are to update or to quit.
// - "whats-new": what changed in this version, from CHANGELOG.md (bundled at build time).
import { invoke } from "@tauri-apps/api/core";
import changelog from "../../CHANGELOG.md?raw";
import { customScrollbar } from "./scrollbar";
import "./notice.css";

type Mode = { mode: "required" | "whats-new"; version: string };

const title = document.getElementById("title")!;
const content = document.getElementById("content")!;
const actions = document.getElementById("actions")!;

function button(label: string, kind: "primary" | "secondary", onClick: (b: HTMLButtonElement) => void) {
  const b = document.createElement("button");
  b.textContent = label;
  b.className = kind;
  b.addEventListener("click", () => onClick(b));
  actions.append(b);
  return b;
}

/** The changelog section for one version: the lines between its "## x.y.z" and the next. */
function sectionFor(version: string): string[] {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((l) => l.trim() === `## ${version}`);
  if (start < 0) return [];
  const end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
  return lines.slice(start + 1, end < 0 ? undefined : end).filter((l) => l.trim());
}

/** "**bold** and `code`" as DOM nodes; everything else stays plain text. */
function inline(text: string): Node[] {
  return text.split(/(\*\*[^*]+\*\*|`[^`]+`)/).filter(Boolean).map((part) => {
    if (part.startsWith("**")) {
      const b = document.createElement("strong");
      b.textContent = part.slice(2, -2);
      return b;
    }
    if (part.startsWith("`")) {
      const c = document.createElement("code");
      c.textContent = part.slice(1, -1);
      return c;
    }
    return document.createTextNode(part);
  });
}

/** Two-level bullet list ("- item", "  - sub item"). */
function renderList(lines: string[]): HTMLElement {
  const list = document.createElement("ul");
  let last: HTMLLIElement | null = null;
  for (const line of lines) {
    const m = line.match(/^(\s*)- (.*)$/);
    if (!m) continue;
    const li = document.createElement("li");
    li.append(...inline(m[2]));
    if (m[1].length > 0 && last) {
      let sub = last.querySelector("ul");
      if (!sub) last.append((sub = document.createElement("ul")));
      sub.append(li);
    } else {
      list.append(li);
      last = li;
    }
  }
  return list;
}

function showRequired() {
  title.textContent = "Update required";
  const p = document.createElement("p");
  p.textContent =
    "This version of Magma MCSR is no longer supported. Update to keep syncing replays. Nothing waiting to upload will be lost.";
  content.append(p);
  const quit = button("Quit", "secondary", () => invoke("notice_action", { action: "quit" }).catch(console.error));
  const update = button("Update now", "primary", () => {
    update.disabled = quit.disabled = true;
    update.textContent = "Updating…";
    invoke("notice_action", { action: "update" }).catch((e) => {
      console.error(e);
      update.disabled = quit.disabled = false;
      update.textContent = "Try again";
    });
  });
  update.focus();
}

function showWhatsNew(version: string) {
  title.textContent = `What's new in ${version}`;
  const lines = sectionFor(version);
  if (lines.length) {
    content.append(renderList(lines));
  } else {
    const p = document.createElement("p");
    p.textContent = "See the release notes on GitHub.";
    content.append(p);
  }
  button("All versions on GitHub", "secondary", () => invoke("notice_action", { action: "github" }).catch(console.error));
  button("Got it", "primary", () => invoke("notice_action", { action: "close" }).catch(console.error)).focus();
}

// Show the page only once the background image is decoded (see notice.css).
const background = new Image();
background.src = "/notice-bg.webp";
const ready = background.decode().catch(() => {});

customScrollbar(content);

invoke<Mode>("notice_mode")
  .then(({ mode, version }) => (mode === "required" ? showRequired() : showWhatsNew(version)))
  .catch(console.error)
  .finally(() => ready.then(() => document.body.classList.add("ready")));

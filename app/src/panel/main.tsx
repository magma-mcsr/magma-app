// Replay sync settings panel. Runs as its own local webview over the website's right edge.
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./panel.css";

type Tracked = { uuid: string; nickname: string };
type Settings = {
  players: Tracked[];
  keepCopy: boolean;
  copyFolder: string | null;
  startWithWindows: boolean;
  runInTray: boolean;
  paused: boolean;
};
type Init = { settings: Settings; version: string; defaultFolder: string | null };

const LEARN_MORE = "https://magmamcsr.com/privacy";
// Both players are in the name, so the file is identifiable when several players are synced.
const EXAMPLE_FILE = "2026-10-16_14201972_Feinberg_vs_subdas.rrf";

function Avatar({ uuid, name }: { uuid: string; name: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <span className="avatar fallback">{name.charAt(0).toUpperCase()}</span>;
  return <img className="avatar" src={`https://minotar.net/helm/${uuid}/32.png`} alt="" onError={() => setFailed(true)} />;
}

/** Small info icon with a hover tooltip. */
function InfoTip({ text }: { text: string }) {
  return (
    <span className="tip" tabIndex={0} aria-label={text}>
      <svg viewBox="0 0 16 16" aria-hidden>
        <circle cx="8" cy="8" r="6.5" />
        <path d="M8 7.2v3.8M8 5h.01" />
      </svg>
      <span className="bubble" role="tooltip">
        {text}
      </span>
    </span>
  );
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button className={`toggle ${checked ? "on" : ""}`} role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}>
      <span />
    </button>
  );
}

/** Name input that looks the player up on the official API before adding them. */
function AddPlayer({ onAdded, autoFocus }: { onAdded: (s: Settings) => void; autoFocus?: boolean }) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const p = await invoke<Tracked>("lookup_player", { name });
      onAdded(await invoke<Settings>("add_player", { player: { uuid: p.uuid, nickname: p.nickname } }));
      setName("");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="lookup" onSubmit={submit}>
      <input
        value={name}
        onChange={(e) => setName(e.currentTarget.value)}
        placeholder="+ Add player"
        spellCheck={false}
        autoFocus={autoFocus}
        disabled={busy}
      />
      {error && <div className="error">{error}</div>}
    </form>
  );
}

function Players({ settings, setSettings }: { settings: Settings; setSettings: (s: Settings) => void }) {
  const [order, setOrder] = useState(settings.players);
  // Index being dragged, and the gap it would drop into (0 = before the first row).
  const [dragging, setDragging] = useState<number | null>(null);
  const [gap, setGap] = useState<number | null>(null);
  const dragFrom = useRef<number | null>(null);
  useEffect(() => setOrder(settings.players), [settings.players]);

  function endDrag() {
    dragFrom.current = null;
    setDragging(null);
    setGap(null);
  }

  /** Top half of a row means "insert above it", bottom half means "insert below it". */
  function onDragOver(e: React.DragEvent, i: number) {
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    setGap(e.clientY < rect.top + rect.height / 2 ? i : i + 1);
  }

  function onDrop() {
    const from = dragFrom.current;
    const to = gap;
    endDrag();
    if (from == null || to == null) return;
    // Removing the row first shifts every later gap up by one.
    const target = to > from ? to - 1 : to;
    if (target === from) return;
    const next = [...order];
    const [moved] = next.splice(from, 1);
    next.splice(target, 0, moved);
    setOrder(next);
    invoke<Settings>("reorder_players", { uuids: next.map((p) => p.uuid) }).then(setSettings);
  }

  // A gap right next to the dragged row would not move anything, so it shows no line.
  const showGap = (g: number) => gap === g && dragging != null && g !== dragging && g !== dragging + 1;

  return (
    <section>
      <h2>
        Players
        <InfoTip text="When the download limit is reached, players higher in the list are downloaded first." />
      </h2>
      {order.map((p, i) => (
        <div
          key={p.uuid}
          className={[
            "row",
            dragging === i && "dragging",
            showGap(i) && "drop-above",
            i === order.length - 1 && showGap(order.length) && "drop-below",
          ]
            .filter(Boolean)
            .join(" ")}
          draggable
          onDragStart={(e) => {
            dragFrom.current = i;
            setDragging(i);
            // Chromium only starts a drag when it carries some data.
            e.dataTransfer.setData("text/plain", p.uuid);
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={(e) => onDragOver(e, i)}
          onDrop={onDrop}
          onDragEnd={endDrag}
        >
          <span className="handle" aria-hidden>
            ⠿
          </span>
          <Avatar uuid={p.uuid} name={p.nickname} />
          <span className="name">{p.nickname}</span>
          <button
            className="remove"
            aria-label={`Stop syncing ${p.nickname}`}
            onClick={() => invoke<Settings>("remove_player", { uuid: p.uuid }).then(setSettings)}
          >
            ✕
          </button>
        </div>
      ))}
      <AddPlayer onAdded={setSettings} autoFocus={order.length === 0} />
    </section>
  );
}

function Panel({ init }: { init: Init }) {
  const [settings, setSettings] = useState(init.settings);
  const empty = settings.players.length === 0;

  useEffect(() => {
    const unlisten = listen<Settings>("settings-changed", ({ payload }) => setSettings(payload));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && invoke("close_panel");
    window.addEventListener("keydown", onKey);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const set = (cmd: string, args: Record<string, unknown>) => invoke<Settings>(cmd, args).then(setSettings).catch(console.error);
  const learnMore = (
    <button className="link" onClick={() => invoke("open_link", { url: LEARN_MORE })}>
      Learn more
    </button>
  );

  return (
    <div className="panel">
      <header>
        <span className="title">Settings</span>
        <span className="spacer" />
        <button className="close" aria-label="Close" onClick={() => invoke("close_panel")}>
          ✕
        </button>
      </header>

      <div className="body">
        <section className="group">
          <div className="group-head">
            <h2 className="group-title">Replay sync</h2>
            {!empty && (
              // Master switch for sync only: on = syncing, off = paused.
              <Toggle label="Sync replays" checked={!settings.paused} onChange={(on) => set("set_paused", { paused: !on })} />
            )}
          </div>
          {empty && (
            // First run: the full explanation, shown once before anything is downloaded.
            <p className="notice intro">
              Add the players whose replays you want to keep. New replays are downloaded in the background and uploaded to
              Magma for analysis. Analyses are public. {learnMore}
            </p>
          )}
          <Players settings={settings} setSettings={setSettings} />
          <div className="setting">
            <span>Keep a copy of replays</span>
            <Toggle label="Keep a copy of replays" checked={settings.keepCopy} onChange={(v) => set("set_keep_copy", { enabled: v })} />
          </div>
          {settings.keepCopy && (
            // Belongs to the toggle above, so it is indented under it.
            <div className="sub">
              <div className="folder">
                {/* The folder icon is a familiar enough convention to stand alone; the tooltip names it. */}
                <span className="tip">
                  <button className="icon-button" aria-label="Change folder" onClick={() => set("choose_folder", {})}>
                    <svg viewBox="0 0 16 16" aria-hidden>
                      <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.4 1.5h5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z" />
                    </svg>
                  </button>
                  <span className="bubble short" role="tooltip">
                    Change folder
                  </span>
                </span>
                <span className="path">{settings.copyFolder ?? init.defaultFolder}</span>
              </div>
              <div className="example">{EXAMPLE_FILE}</div>
            </div>
          )}
        </section>

        <section className="group divided">
          <div className="group-head">
            <h2 className="group-title">App</h2>
          </div>
          <div className="setting">
            <span>Start with Windows</span>
            <Toggle label="Start with Windows" checked={settings.startWithWindows} onChange={(v) => set("set_start_with_windows", { enabled: v })} />
          </div>
          <div className="setting">
            <span>Keep running in the tray</span>
            <Toggle label="Keep running in the tray" checked={settings.runInTray} onChange={(v) => set("set_run_in_tray", { enabled: v })} />
          </div>
        </section>

        <footer>
          {/* While the list is empty, the same sentence is already in the intro above. */}
          <span>{!empty && <>Replays are uploaded to Magma and analyses are public. {learnMore}</>}</span>
          <button className="link version" onClick={() => invoke("show_whats_new").catch(console.error)}>
            v{init.version} · What's new
          </button>
        </footer>
      </div>
    </div>
  );
}

invoke<Init>("panel_init").then((init) =>
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <Panel init={init} />
    </React.StrictMode>,
  ),
);

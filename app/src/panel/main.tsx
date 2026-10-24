// The app's own view (the "App" tab): first-run welcome, then your recent matches, the players
// to sync and the settings. Runs as its own local webview over the website.
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { customScrollbar } from "../scrollbar";
import "../fonts.css";
import "./panel.css";

type Tracked = { uuid: string; nickname: string };
type Settings = {
  players: Tracked[];
  keepCopy: boolean;
  copyFolder: string | null;
  startWithWindows: boolean;
  runInTray: boolean;
  paused: boolean;
  network: string | null;
  autoTrackedNotice: boolean;
};
type Connection = { name: string; address: string };
type SyncStatus = { state: string; label: string };
type Init = { settings: Settings; version: string; defaultFolder: string | null; status: SyncStatus; setupPending: boolean };
type OwnMatch = {
  id: number;
  date: number;
  opponent: string | null;
  outcome: "won" | "lost" | "draw";
  forfeited: boolean;
  state: "analyzed" | "analyzing" | "uploading" | "waiting" | "no_replay";
};

// Until magmamcsr.com has a privacy page, this explains what the app does and sends.
const LEARN_MORE = "https://github.com/magma-mcsr/magma-app#what-it-does";
// Both players are in the name, so the file is identifiable when several players are synced.
const EXAMPLE_FILE = "2026-10-16_14201972_Feinberg_vs_subdas.rrf";

function Avatar({ uuid, name }: { uuid: string; name: string }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <span className="avatar fallback">{name.charAt(0).toUpperCase()}</span>;
  return <img className="avatar" src={`https://minotar.net/helm/${uuid}/32.png`} alt="" onError={() => setFailed(true)} />;
}

/** Small info icon with a hover tooltip. */
/** Ask for the first player's newest matches right away; the label says so for a moment. */
function CheckNow({ onChecked }: { onChecked?: () => void }) {
  const [busy, setBusy] = useState(false);
  const click = () => {
    setBusy(true);
    invoke("check_now").catch(console.error);
    setTimeout(() => {
      setBusy(false);
      onChecked?.();
    }, 3000);
  };
  return (
    <button className="link" disabled={busy} onClick={click}>
      {busy ? "Checking…" : "Check now"}
    </button>
  );
}

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

/** Which network connection sync goes through. The list is read again whenever it is opened,
 *  so a connection plugged in later shows up. */
function NetworkPicker({ value, onChange }: { value: string | null; onChange: (name: string | null) => void }) {
  const [options, setOptions] = useState<Connection[]>([]);
  const refresh = () => invoke<Connection[]>("network_options").then(setOptions).catch(console.error);
  useEffect(() => {
    refresh();
  }, []);
  const missing = value !== null && !options.some((o) => o.name === value);
  return (
    <select className="select" aria-label="Network" value={value ?? ""} onFocus={refresh} onChange={(e) => onChange(e.target.value || null)}>
      <option value="">Automatic</option>
      {options.map((o) => (
        <option key={o.name} value={o.name}>
          {o.name} ({o.address})
        </option>
      ))}
      {missing && <option value={value}>{value} (not connected)</option>}
    </select>
  );
}

/** Name input that looks the player up on the official API before adding them. */
function AddPlayer({ onAdded, first }: { onAdded: (s: Settings) => void; first?: boolean }) {
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
      // The first player is usually yourself: show where your matches will appear
      if (first) invoke("show_player_matches", { nickname: p.nickname }).catch(console.error);
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
        placeholder={first ? "Your Minecraft name" : "+ Add player"}
        spellCheck={false}
        autoFocus={first}
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
      <AddPlayer onAdded={setSettings} first={order.length === 0} />
    </section>
  );
}

const LEARN = (
  <button className="link" onClick={() => invoke("open_link", { url: LEARN_MORE })}>
    Learn more
  </button>
);

function ago(unix: number): string {
  const s = Math.max(0, Date.now() / 1000 - unix);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** What a match page on Magma shows, as a small static sample. */
function Preview() {
  const lines: [string, string, boolean][] = [
    ["7:10", "zero cycle failed (died after 6 beds: shot a crystal next to them) · y95 · dragon at 1 HP", true],
    ["8:41", "zero cycle failed (dragon flew) · y90 · dragon at 105 HP", true],
    ["12:04", "one cycle", false],
  ];
  return (
    <div className="preview" aria-hidden>
      <div className="preview-title">Highlights</div>
      {lines.map(([t, text, key]) => (
        <div className="preview-line" key={t}>
          <span className="preview-time">{t}</span>
          <span className={key ? "preview-mark on" : "preview-mark"} />
          <span>{text}</span>
        </div>
      ))}
    </div>
  );
}

/** First run: what the app is for, and the one step to set it up. */
function Welcome({ onSettings }: { onSettings: (s: Settings) => void }) {
  const [name, setName] = useState("");
  const [found, setFound] = useState<Tracked | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<Tracked[]>("detect_accounts")
      .then((list) => {
        if (!list[0]) return;
        setFound(list[0]);
        setName((typed) => typed || list[0].nickname);
      })
      .catch(() => {});
  }, []);
  const isFound = found !== null && found.nickname.toLowerCase() === name.trim().toLowerCase();

  async function track(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const p = await invoke<Tracked>("lookup_player", { name });
      const settings = await invoke<Settings>("add_player", { player: { uuid: p.uuid, nickname: p.nickname } });
      // Ready on the website for when they switch to it
      invoke("show_player_matches", { nickname: p.nickname }).catch(console.error);
      onSettings(settings);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  return (
    <div className="welcome">
      <h1>See your analysis after every match</h1>
      <p className="lead">
        Magma breaks down your MCSR Ranked matches: highlights, how each End fight went, where time was won or lost. This
        app sends your new replays to Magma in the background, so the analysis is on your Matches page about a minute after
        each match.
      </p>
      <Preview />
      <ol className="steps">
        <li>
          <strong>Track yourself</strong>
          <span>Confirm your name below.</span>
        </li>
        <li>
          <strong>Play Ranked</strong>
          <span>The app watches for your new matches while it runs.</span>
        </li>
        <li>
          <strong>Open your Matches page</strong>
          <span>The analysis is there about a minute after the match.</span>
        </li>
      </ol>
      <form className="you" onSubmit={track}>
        <label htmlFor="you">Is this you?</label>
        <div className="you-row">
          {isFound && <Avatar uuid={found!.uuid} name={found!.nickname} />}
          <input
            id="you"
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            placeholder="Your Minecraft name"
            spellCheck={false}
            autoFocus
            disabled={busy}
          />
          <button className="btn primary" type="submit" disabled={busy || !name.trim()}>
            {busy ? "Looking up…" : "Track my matches"}
          </button>
        </div>
        <p className="hint">
          {isFound ? "Found in this computer's Minecraft logs. " : found ? "" : "Type your Minecraft name. "}
          Analyses are public. {LEARN}
        </p>
        {error && <div className="error">{error}</div>}
      </form>
      <button className="link quiet" onClick={() => invoke<Settings>("dismiss_welcome").then(onSettings).catch(console.error)}>
        Not now
      </button>
    </div>
  );
}

/** After the first-run setup tracked the name found on this computer: what that does, and the way
 *  to change it or turn it off. Stays until "Got it". */
function SetupNotice({ me, onSettings }: { me: Tracked; onSettings: (s: Settings) => void }) {
  const [changing, setChanging] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function change(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const p = await invoke<Tracked>("lookup_player", { name });
      onSettings(await invoke<Settings>("replace_first_player", { replace: me.uuid, player: { uuid: p.uuid, nickname: p.nickname } }));
      invoke("show_player_matches", { nickname: p.nickname }).catch(console.error);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  return (
    <section className="card setup">
      <h2 className="card-title">Your matches are now analyzed on Magma</h2>
      <p className="setup-lead">
        We found <strong>{me.nickname}</strong> in this computer's Minecraft logs and started tracking that name.
      </p>
      <ul className="how">
        <li>While the app runs, it checks for your new Ranked matches.</li>
        <li>It downloads each replay from MCSR Ranked's public replay server and uploads it to Magma.</li>
        <li>Magma analyzes it, and the analysis is on your Matches page about a minute after the match. Analyses are public.</li>
      </ul>
      {changing ? (
        <form className="you-row" onSubmit={change}>
          <input value={name} onChange={(e) => setName(e.currentTarget.value)} placeholder="Your Minecraft name" spellCheck={false} autoFocus disabled={busy} />
          <button className="btn primary" type="submit" disabled={busy || !name.trim()}>
            {busy ? "Looking up…" : "Save"}
          </button>
          <button className="btn" type="button" onClick={() => setChanging(false)} disabled={busy}>
            Cancel
          </button>
        </form>
      ) : (
        <div className="actions">
          <button className="btn primary" onClick={() => invoke<Settings>("ack_setup_notice").then(onSettings).catch(console.error)}>
            Got it
          </button>
          <button className="btn" onClick={() => setChanging(true)}>
            Change name
          </button>
          <button className="btn" onClick={() => invoke<Settings>("undo_auto_track", { uuid: me.uuid }).then(onSettings).catch(console.error)}>
            Turn off
          </button>
          <span className="spacer" />
          {LEARN}
        </div>
      )}
      {error && <div className="error">{error}</div>}
    </section>
  );
}

const STATE_LABEL: Record<OwnMatch["state"], string> = {
  analyzed: "Open analysis",
  analyzing: "Analyzing",
  uploading: "Uploading",
  waiting: "Waiting",
  no_replay: "No replay",
};

/** The first player's newest matches and how far each one got. */
function RecentMatches({ me }: { me: Tracked }) {
  const [list, setList] = useState<OwnMatch[] | null>(null);
  const load = () =>
    invoke<OwnMatch[]>("recent_own")
      .then(setList)
      .catch(() => setList((l) => l ?? []));
  useEffect(() => {
    setList(null);
    load();
    const timer = window.setInterval(load, 30_000);
    return () => window.clearInterval(timer);
  }, [me.uuid]);

  return (
    <section className="card">
      <div className="card-head">
        <h2 className="card-title">Your recent matches</h2>
        <span className="card-note">{me.nickname}</span>
        <span className="spacer" />
        <CheckNow onChecked={load} />
      </div>
      {list === null ? (
        <p className="muted">Loading…</p>
      ) : list.length === 0 ? (
        <p className="muted">No matches yet. Play a Ranked match and it shows up here.</p>
      ) : (
        <ul className="matches">
          {list.map((m) => (
            <li key={m.id}>
              <button
                className="match"
                disabled={m.state !== "analyzed"}
                onClick={() => invoke("show_player_matches", { nickname: me.nickname, matchId: m.id, show: true }).catch(console.error)}
              >
                <span className={`outcome ${m.outcome}`}>{m.outcome === "won" ? "Won" : m.outcome === "lost" ? "Lost" : "Draw"}</span>
                <span className="vs">
                  vs {m.opponent ?? "?"}
                  {m.forfeited && <span className="muted"> · forfeit</span>}
                </span>
                <span className="when">{ago(m.date)}</span>
                <span className={`state ${m.state}`}>{STATE_LABEL[m.state]}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <p className="card-foot">
        Checked every 15 seconds. A match you just finished is usually analyzed within a minute.
      </p>
    </section>
  );
}

type Page = "replays" | "settings" | "about";
const PAGES: { id: Page; label: string }[] = [
  { id: "replays", label: "Replays" },
  { id: "settings", label: "Settings" },
  { id: "about", label: "About" },
];
const REPO = "https://github.com/magma-mcsr/magma-app";

function Panel({ init }: { init: Init }) {
  const [settings, setSettings] = useState(init.settings);
  const [status, setStatus] = useState(init.status);
  const [page, setPage] = useState<Page>("replays");
  // The first-run setup may still be looking for this computer's Minecraft name
  const [pending, setPending] = useState(init.setupPending);
  const empty = settings.players.length === 0;
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => customScrollbar(body.current!), []);
  // A block body: scrollTo returns a promise in newer WebView2, and an effect may only return a cleanup function
  useEffect(() => {
    body.current?.scrollTo(0, 0);
  }, [page]);

  useEffect(() => {
    const a = listen<Settings>("settings-changed", ({ payload }) => setSettings(payload));
    const b = listen<SyncStatus>("sync-status", ({ payload }) => setStatus(payload));
    const c = listen("setup-done", () => setPending(false));
    return () => {
      a.then((f) => f());
      b.then((f) => f());
      c.then((f) => f());
    };
  }, []);

  const set = (cmd: string, args: Record<string, unknown>) => invoke<Settings>(cmd, args).then(setSettings).catch(console.error);
  const link = (url: string, text: string) => (
    <button className="link" onClick={() => invoke("open_link", { url }).catch(console.error)}>
      {text}
    </button>
  );

  const replays = (
    <>
      <header className="page-head">
        <h1>Replays</h1>
        <span className={`status ${status.state}`}>{status.label}</span>
        <span className="spacer" />
        {/* Master switch for sync only: on = syncing, off = paused. */}
        <Toggle label="Sync replays" checked={!settings.paused} onChange={(on) => set("set_paused", { paused: !on })} />
      </header>

      {settings.autoTrackedNotice && <SetupNotice me={settings.players[0]} onSettings={setSettings} />}

      <RecentMatches me={settings.players[0]} />

      <section className="card">
        <Players settings={settings} setSettings={setSettings} />
      </section>

      <section className="card">
        <h2 className="card-title">Sync options</h2>
        <div className="setting">
          <span>
            Keep a copy of replays{" "}
            <InfoTip text="Saved to your MCSR Ranked replay folder when the app finds it, so they show up under My Replays in game." />
          </span>
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
        <div className="setting">
          <span>
            Network{" "}
            <InfoTip text="Which connection downloads and uploads go through. Pick a different one than other computers syncing on the same network: each connection has its own download limit." />
          </span>
          <NetworkPicker value={settings.network} onChange={(name) => set("set_network", { name })} />
        </div>
      </section>
    </>
  );

  const appSettings = (
    <>
      <header className="page-head">
        <h1>Settings</h1>
      </header>
      <section className="card">
        <div className="setting">
          <span>Start with Windows</span>
          <Toggle label="Start with Windows" checked={settings.startWithWindows} onChange={(v) => set("set_start_with_windows", { enabled: v })} />
        </div>
        <div className="setting">
          <span>
            Keep running in the tray{" "}
            <InfoTip text="Closing the window keeps replay sync running in the background. Quit from the tray icon." />
          </span>
          <Toggle label="Keep running in the tray" checked={settings.runInTray} onChange={(v) => set("set_run_in_tray", { enabled: v })} />
        </div>
      </section>
    </>
  );

  const about = (
    <>
      <header className="page-head">
        <h1>About</h1>
      </header>
      <section className="card">
        <div className="setting">
          <span>Magma MCSR app v{init.version}</span>
          <button className="link" onClick={() => invoke("show_whats_new").catch(console.error)}>
            What's new
          </button>
        </div>
        <p className="about-text">
          The Website tab is magmamcsr.com, the same as in your browser. The app adds replay sync: it downloads the
          replays of the players you list from MCSR Ranked's public replay server and uploads them to Magma for analysis.
          Analyses are public.
        </p>
        <p className="about-text">
          It is open source, so you can check exactly what it reads, downloads and sends. {link(LEARN_MORE, "What it does")}
          {" · "}
          {link(REPO, "Source code")}
          {" · "}
          {link("https://magmamcsr.com/", "magmamcsr.com")}
        </p>
        <p className="about-text muted">Magma MCSR is a community project, not affiliated with MCSR Ranked.</p>
      </section>
    </>
  );

  return (
    <div className={empty ? "panel" : "panel with-nav"}>
      {!empty && (
        // Three plain text entries: short words read faster than icons here.
        <nav className="sidebar" aria-label="App">
          {PAGES.map((p) => (
            <button key={p.id} className="nav-item" aria-current={page === p.id ? "page" : undefined} onClick={() => setPage(p.id)}>
              <span>{p.label}</span>
              {p.id === "replays" && <span className={`nav-dot ${status.state}`} title={status.label} />}
            </button>
          ))}
        </nav>
      )}
      <div className="body-row">
        <div className="body" ref={body}>
          <div className="page">
            {empty && pending ? (
              <p className="muted looking">Looking for your Minecraft account on this computer…</p>
            ) : empty ? (
              <Welcome onSettings={setSettings} />
            ) : page === "replays" ? (
              replays
            ) : page === "settings" ? (
              appSettings
            ) : (
              about
            )}
          </div>
        </div>
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

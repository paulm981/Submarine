import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, Home, Loader2, PanelRightClose, PanelRightOpen, X } from "lucide-react";
import { AiTab, aiMarker, openAiTrust, sharedWith, trustTargetFor, useAi } from "../util/ai";
import {
  PANE_MIN_WIDTH, aiStarted, aiPaneState, choosePaneAi, closePaneHere, leftView, resetAiPane, setPaneFolded, setPaneHere,
  setPaneWidth, showHomeTab, useAiPane,
} from "../ui/aiPane";
import { appHotkey, claimTerminal, dockedTerminal, setAiPaneKeyLive, useDockVersion } from "../ui/terminalDock";
import { IS_ANDROID } from "../util/platform";
import { PendingCard, levelOf } from "./AiPanel";

// ─────────────────────────────────────────────────────────────────────────────
// The AI side pane (#100): an AI tab beside the server tab you're working in,
// so working with it doesn't mean going back and forth to Home.
//
// It follows sharing: on a server tab shared with an AI tab, or a Wall with a
// server shared with one, it's on the right, open or folded to a thin bar as
// you last left it (Ctrl+Shift+L, or a click). Anywhere else there's none,
// unless the title bar's button opens it there; it then stays until it's
// closed or you leave that tab.
//
// It shows the AI tab's own terminal, moved out of Home (src/ui/terminalDock.ts,
// as a Wall borrows a server's terminal) while the pane is open and back when
// it folds or closes, or you go to Home. The conversation is only ever in one
// place. The header says which AI tab it is, whether this tab is shared with
// it, and has its requests' Allow and Deny — the AI panel's own cards, with
// the same care: a single click only, never a key, not just after they move.
//
// The keyboard goes into the AI only when you put it there (#116): a click
// in its terminal, or the pane opened with a key on its own button. Opening
// or folding it otherwise (a click, Ctrl+Shift+L, an AI tool started from
// the + menu) leaves the keyboard where it was: the AI tool's own questions
// are answered with Enter, so typing meant for the server mustn't land
// there. Of the two terminals, the one with the keyboard is bright and the
// other dimmed.
// ─────────────────────────────────────────────────────────────────────────────

/// What's on screen, as the pane needs it (DesktopApp).
export interface AiPaneView {
  /// The view on screen (DesktopApp's activeView): "home", a Wall's id, a
  /// server tab's session id, or a page such as Settings.
  view: string;
  /// The server tab on screen, if one is.
  session: { id: string; name: string; ssh: boolean; connected: boolean; terminalId: string } | null;
  /// The Wall on screen, if one is: the sessions on it.
  wallSessions: string[] | null;
}

interface Model {
  /// The pane can be on this view at all.
  allowed: boolean;
  /// The AI tabs this view shares something with.
  relevant: AiTab[];
  /// Every AI tab, and one still starting.
  all: AiTab[];
  /// Opened here by the button, nothing shared needed.
  here: boolean;
  /// The AI tab it shows.
  current: AiTab | undefined;
  /// It's on screen, open or folded.
  shows: boolean;
  /// It's open (not folded).
  open: boolean;
}

const useModel = (v: AiPaneView): Model => {
  const ai = useAi();
  const pane = useAiPane();
  const allowed = !IS_ANDROID && ai.enabled && v.view !== "home";
  const relevant = v.session
    ? sharedWith(ai, v.session.id)
    : v.wallSessions
      ? ai.tabs.filter((t) => t.servers.some((s) => v.wallSessions!.includes(s.session_id)))
      : [];
  const starting = pane.starting && !ai.tabs.some((t) => t.id === pane.starting!.id)
    ? { id: pane.starting.id, label: pane.starting.label, servers: [], intruder_at: null }
    : null;
  const all = starting ? [...ai.tabs, starting] : ai.tabs;
  const here = pane.here === v.view;
  const pick = all.find((t) => t.id === pane.chosen);
  const current = pick && (here || relevant.includes(pick)) ? pick : relevant[0] ?? (here ? pick ?? all[0] : undefined);
  const shows = allowed && !!current;
  // Where nothing is shared it's open or not there: folding closes it.
  const open = shows && (!pane.folded || relevant.length === 0);
  return { allowed, relevant, all, here, current, shows, open };
};

/// The title bar's button, Ctrl+Shift+L and the fold bar: open the pane, or
/// fold it (close it, where it was only opened here).
const toggle = (v: AiPaneView, m: Model) => {
  if (!m.allowed) return false;
  if (m.shows) {
    if (m.relevant.length === 0) closePaneHere();
    else if (m.open) { setPaneFolded(true); setPaneHere(null); }
    else setPaneFolded(false);
    return !m.open;
  }
  if (m.all.length === 0) return false;
  setPaneHere(v.view);
  setPaneFolded(false);
  return true;
};

/// Is the keyboard in a field (not a terminal)? Ctrl+Shift+L isn't taken
/// from it.
const inField = () => {
  const a = document.activeElement as HTMLElement | null;
  if (!a || a.closest(".xterm")) return false;
  return a.tagName === "INPUT" || a.tagName === "TEXTAREA" || a.tagName === "SELECT" || a.isContentEditable;
};

/// The title bar's button: shows the AI beside this tab, or folds it. Only
/// where it can: AI tools on, an AI tab open, not in Home.
export const AiPaneButton = ({ v }: { v: AiPaneView }) => {
  const m = useModel(v);
  if (!m.allowed || m.all.length === 0) return null;
  const label = m.open ? "Fold the AI pane" : "Show the AI beside this tab";
  return (
    <button
      type="button"
      onClick={() => toggle(v, m)}
      title={`${label} (Ctrl+Shift+L)`}
      aria-label={label}
      aria-pressed={m.open}
      className={`flex items-center justify-center w-7 h-7 rounded-full transition-all ${
        m.open
          ? "bg-primary/15 border border-primary/40 text-primary"
          : "bg-white/[0.06] border border-white/10 text-zinc-400 hover:bg-white/[0.1] hover:text-white"
      }`}
    >
      {m.open ? <PanelRightClose size={13} /> : <PanelRightOpen size={13} />}
    </button>
  );
};

export const AiSidePane = ({ v, onHome, onGoToServer }: {
  v: AiPaneView;
  /// Back to Home, where the AI tab is.
  onHome: () => void;
  onGoToServer: (sessionId: string) => void;
}) => {
  const ai = useAi();
  const pane = useAiPane();
  const m = useModel(v);
  const { current } = m;
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [current?.id, v.view]);

  // Opened on one tab only: gone when it's left.
  useEffect(() => { leftView(v.view); }, [v.view]);
  // Unsharing takes the pane off this tab, wherever it's done (here, the
  // tab's own button, the AI panel): the AI goes on in Home.
  const sharedHere = useRef({ view: v.view, n: m.relevant.length });
  useEffect(() => {
    const before = sharedHere.current;
    sharedHere.current = { view: v.view, n: m.relevant.length };
    if (before.view === v.view && before.n > 0 && m.relevant.length === 0 && aiPaneState().here === v.view) setPaneHere(null);
  }, [v.view, m.relevant.length]);
  // Nothing carries over to the next profile.
  useEffect(() => () => resetAiPane(), []);
  // One started from a server tab has started: shown as any other.
  const started = pane.starting && ai.tabs.some((t) => t.id === pane.starting!.id) ? pane.starting.id : null;
  useEffect(() => { if (started) aiStarted(started); }, [started]);

  // Where the keyboard goes as it opens or folds: into the AI only when you
  // open it with a key on its own button, back to this tab's terminal when
  // you fold it from inside. Otherwise it stays where it was.
  const paneRef = useRef<HTMLElement | null>(null);
  // Keys pressed and clicks anywhere, counted: the AI takes the keyboard
  // only if nothing was pressed or clicked since it was asked to.
  const inputs = useRef(0);
  useEffect(() => {
    const count = () => { inputs.current++; };
    window.addEventListener("keydown", count, true);
    window.addEventListener("pointerdown", count, true);
    return () => {
      window.removeEventListener("keydown", count, true);
      window.removeEventListener("pointerdown", count, true);
    };
  }, []);
  // The count when the AI was asked to take the keyboard, until it has.
  const focusAi = useRef<number | null>(null);
  const askFocus = () => { focusAi.current = inputs.current; };
  /// `intoAi`: the pane's own button, pressed with a key: opening it puts
  /// the keyboard in the AI.
  const act = (intoAi: boolean) => {
    const fromInside = !!paneRef.current?.contains(document.activeElement);
    const opening = toggle(v, m);
    if (opening && intoAi) askFocus();
    else if (!opening && fromInside && v.session) {
      requestAnimationFrame(() => dockedTerminal(v.session!.terminalId)?.term.focus());
    }
  };
  const keysIn = useKeysIn(paneRef, m.open);
  const actRef = useRef(act);
  actRef.current = act;
  // Ctrl+Shift+L is taken from the terminals only while it does something
  // here (see appHotkey); this pane isn't mounted in a narrow window.
  const keyLive = m.allowed && (m.shows || m.all.length > 0);
  useLayoutEffect(() => { setAiPaneKeyLive(keyLive); }, [keyLive]);
  useLayoutEffect(() => () => setAiPaneKeyLive(false), []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (appHotkey(e) !== "aiPane") return;
      if (e.repeat || inField()) return;
      if (document.querySelector('[aria-modal="true"]')) return;
      e.preventDefault();
      actRef.current(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // The window's width, to keep the pane from taking all of it.
  const [winW, setWinW] = useState(() => window.innerWidth);
  useEffect(() => {
    const onResize = () => setWinW(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  const maxW = Math.max(PANE_MIN_WIDTH, winW - 420);
  const width = Math.min(pane.width, maxW);

  if (!m.shows || !current) return null;
  const waiting = ai.pending.filter((p) => p.tab_id === current.id);

  if (!m.open) {
    return (
      <button
        type="button"
        onClick={(e) => act(e.detail === 0)}
        title={`Open the AI pane: ${current.label} (Ctrl+Shift+L)`}
        aria-label={`Open the AI pane: ${current.label}${waiting.length ? `, ${waiting.length} waiting for you` : ""}`}
        className="relative w-7 shrink-0 min-h-0 flex flex-col items-center gap-2 pt-3 bg-[#111114] border-l border-white/10 text-zinc-400 hover:text-white hover:bg-white/[0.04] focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
      >
        <PanelRightOpen size={14} />
        <Bot size={14} className="text-primary" />
        {waiting.length > 0 && (
          <span className="min-w-[14px] h-[14px] px-1 rounded-full bg-primary text-[9px] font-bold text-black flex items-center justify-center leading-none" aria-hidden="true">
            {waiting.length}
          </span>
        )}
        <span className="text-[10.5px] font-semibold tracking-wide [writing-mode:vertical-rl] truncate max-h-[50vh]">{current.label}</span>
      </button>
    );
  }

  const run = (cmd: string, args: Record<string, unknown>, then?: () => void) => {
    setError(null);
    invoke(cmd, args).then(then, (e) => setError(String(e)));
  };
  const starting = !ai.tabs.some((t) => t.id === current.id);
  const shared = !!v.session && current.servers.some((s) => s.session_id === v.session!.id);
  const onlyHere = m.relevant.length === 0;

  return (
    <aside
      ref={paneRef}
      aria-label={`AI pane: ${current.label}`}
      style={{ width }}
      className="relative shrink-0 min-h-0 flex flex-col bg-[#111114] border-l border-white/10 text-[12px]"
    >
      <Resizer width={width} max={maxW} />
      <div className="h-11 shrink-0 pl-3 pr-1.5 flex items-center gap-2 border-b border-white/5">
        <Bot size={15} className="text-primary shrink-0" />
        {m.all.length > 1 ? (
          <select
            value={current.id}
            onChange={(e) => {
              const id = e.target.value;
              choosePaneAi(id);
              // One this tab shares nothing with stays shown here.
              if (!m.relevant.some((t) => t.id === id)) setPaneHere(v.view);
            }}
            aria-label="Which AI tab"
            title="Which AI tab"
            className="min-w-0 max-w-[50%] h-7 px-1.5 rounded bg-black/30 border border-white/10 text-zinc-100 font-bold tracking-wide outline-none focus:border-primary/50"
          >
            {m.all.map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}
          </select>
        ) : (
          <span className="font-bold text-zinc-100 tracking-wide truncate">{current.label}</span>
        )}
        {starting && <Loader2 size={12} className="shrink-0 animate-spin text-zinc-500" aria-label="Starting" />}
        <span className="flex-1" />
        <button
          type="button"
          onClick={() => { showHomeTab(current.id); onHome(); }}
          title={`Back to Home, where ${current.label} runs`}
          className="h-7 px-2 rounded flex items-center gap-1.5 text-zinc-400 hover:text-white hover:bg-white/10 text-[11px] font-semibold"
        >
          <Home size={13} /> Back to Home
        </button>
        <button
          type="button"
          onClick={(e) => act(e.detail === 0)}
          title={onlyHere ? "Close the AI pane (Ctrl+Shift+L)" : "Fold the AI pane (Ctrl+Shift+L)"}
          aria-label={onlyHere ? "Close the AI pane" : "Fold the AI pane"}
          className="w-7 h-7 flex items-center justify-center rounded text-zinc-400 hover:text-white hover:bg-white/10"
        >
          {onlyHere ? <X size={14} /> : <PanelRightClose size={14} />}
        </button>
      </div>

      <ShareLine
        v={v}
        tab={current}
        shared={shared}
        starting={starting}
        onShare={() => run("ai_share", { tabId: current.id, sessionId: v.session!.id, name: v.session!.name })}
        onUnshare={() => run("ai_unshare", { sessionId: v.session!.id, tabId: current.id })}
      />

      {/* Its requests, as in the AI panel. */}
      {waiting.length > 0 && (
        <div className="shrink-0 max-h-[45%] overflow-y-auto custom-scrollbar border-b border-white/5">
          {waiting.map((p) => (
            <PendingCard
              key={p.id}
              p={p}
              level={levelOf(ai, p.tab_id, p.session_id)}
              decide={(allow, note, password) => run("ai_decide", { id: p.id, allow, note: note ?? null, password: password || null })}
              act={run}
              onGoToServer={onGoToServer}
            />
          ))}
        </div>
      )}
      {error && <p className="shrink-0 px-3 py-1.5 text-[11px] text-red-400 border-b border-white/5 break-words">{error}</p>}

      <AiTerminalSlot terminalId={current.id} wantFocus={focusAi} inputs={inputs} dim={keysIn === "other"} />
    </aside>
  );
};

/// Whether this tab is shared with the AI, and Share or Unshare.
const ShareLine = ({ v, tab, shared, starting, onShare, onUnshare }: {
  v: AiPaneView; tab: AiTab; shared: boolean; starting: boolean; onShare: () => void; onUnshare: () => void;
}) => {
  const btn = "shrink-0 h-6 px-2 rounded border text-[10px] font-bold uppercase tracking-wider disabled:opacity-40 disabled:pointer-events-none";
  let text: string;
  let action: React.ReactNode = null;
  // How much it may run here without asking (#115): said, with a click to
  // change it, whenever it's more than "Ask every time".
  const here = v.session ? tab.servers.find((s) => s.session_id === v.session!.id) : undefined;
  const marker = here ? aiMarker(here) : null;
  if (v.session && !v.session.ssh) {
    text = "Remote Desktop, VNC and telnet tabs can't be shared with AI.";
  } else if (v.session) {
    text = shared
      ? `This tab is shared with ${tab.label}: it can ask to run commands here.`
      : `This tab isn't shared with ${tab.label}.`;
    action = shared ? (
      <button type="button" onClick={onUnshare} title={`Stop sharing this tab with ${tab.label}`} className={`${btn} border-white/10 text-zinc-300 hover:border-red-400/50 hover:text-red-300`}>
        Unshare
      </button>
    ) : (
      <button
        type="button"
        onClick={onShare}
        disabled={starting || !v.session.connected}
        title={starting
          ? `Once ${tab.label} has started`
          : !v.session.connected
            ? "Once this tab is connected"
            : `Let ${tab.label} ask to run commands on this tab: each one waits for your Allow, unless you have set this server to run commands without asking`}
        className={`${btn} bg-primary/15 border-primary/40 text-primary hover:bg-primary hover:text-zinc-950`}
      >
        Share
      </button>
    );
  } else if (v.wallSessions) {
    const n = new Set(v.wallSessions.filter((sid) => tab.servers.some((s) => s.session_id === sid))).size;
    const all = new Set(v.wallSessions).size;
    text = `${n} of ${all} server${all === 1 ? "" : "s"} on this Wall ${n === 1 ? "is" : "are"} shared with ${tab.label}.`;
  } else {
    text = `Nothing here is shared with ${tab.label}.`;
  }
  return (
    <div className="shrink-0 px-3 py-1.5 flex items-center gap-2 border-b border-white/5 text-[11px] text-zinc-400">
      <span className="flex-1 min-w-0 leading-snug">{text}</span>
      {here?.allow_all && (
        <button
          type="button"
          onClick={() => { invoke("ai_end_allow_all", { tabId: tab.id, sessionId: here.session_id }).catch(() => {}); }}
          title={`Stop allowing everything on this tab for ${tab.label}: its commands go back to this server's approval level`}
          className="shrink-0 h-6 px-2 rounded border border-white/10 text-zinc-300 text-[10px] font-bold tracking-wide hover:border-red-400/50 hover:text-red-300"
        >
          End allow everything
        </button>
      )}
      {here && marker && (
        <button
          type="button"
          onClick={() => openAiTrust(trustTargetFor(here))}
          title={`${marker}${here.allow_all ? " (allowed until this AI tab closes)" : ""}. Click to change how much the AI may run here without asking.`}
          className="shrink-0 h-6 px-2 rounded border border-amber-400/30 bg-amber-400/10 text-amber-300 text-[10px] font-bold tracking-wide hover:bg-amber-400/20"
        >
          {marker}
        </button>
      )}
      {action}
    </div>
  );
};

/// Which terminal has the keyboard while the pane is open: the AI's
/// ("ai"), another one ("other"), or none (a button, a field). With the AI's,
/// the other terminals on screen dim (index.css, `data-ai-keys`); with
/// another, the AI's does.
const useKeysIn = (paneRef: React.RefObject<HTMLElement | null>, open: boolean): "ai" | "other" | null => {
  const [keysIn, setKeysIn] = useState<"ai" | "other" | null>(null);
  useEffect(() => {
    if (!open) {
      setKeysIn(null);
      return;
    }
    const look = () => {
      const term = (document.activeElement as HTMLElement | null)?.closest?.(".xterm");
      setKeysIn(!term ? null : paneRef.current?.contains(term) ? "ai" : "other");
    };
    // Focus leaving says nothing of where it goes: looked at once it's there.
    const later = () => { setTimeout(look, 0); };
    look();
    document.addEventListener("focusin", look);
    document.addEventListener("focusout", later);
    return () => {
      document.removeEventListener("focusin", look);
      document.removeEventListener("focusout", later);
    };
  }, [open, paneRef]);
  useEffect(() => {
    const root = document.documentElement;
    if (keysIn === "ai") root.dataset.aiKeys = "1";
    else delete root.dataset.aiKeys;
    return () => { delete root.dataset.aiKeys; };
  }, [keysIn]);
  return keysIn;
};

/// Where the AI tab's terminal is shown while the pane is open: moved here
/// from Home, and back when this goes. `dim`: another terminal has the
/// keyboard.
const AiTerminalSlot = ({ terminalId, wantFocus, inputs, dim }: {
  terminalId: string;
  /// The input count when the AI was asked to take the keyboard.
  wantFocus: React.MutableRefObject<number | null>;
  inputs: React.MutableRefObject<number>;
  dim: boolean;
}) => {
  useDockVersion();
  const slotRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const slot = slotRef.current;
    if (!slot) return;
    return claimTerminal(terminalId, slot);
  }, [terminalId]);
  const entry = dockedTerminal(terminalId);
  // Opened with a key on its button: the keyboard goes to the AI once its
  // terminal is here, unless a key was pressed or a click made since.
  useEffect(() => {
    if (!entry || wantFocus.current === null) return;
    const id = requestAnimationFrame(() => {
      const asked = wantFocus.current;
      wantFocus.current = null;
      if (asked === inputs.current && !document.querySelector('[aria-modal="true"]')) entry.term.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [entry, wantFocus, inputs]);
  return (
    <div data-ai-pane="" className={`flex-1 min-h-0 relative transition-opacity duration-150 ${dim ? "opacity-60" : ""}`}>
      {/* The padding takes the terminal's own scheme background. */}
      <div ref={slotRef} className="absolute inset-0 p-1.5 select-text" style={{ background: entry?.term.options.theme?.background ?? "#09090b" }} />
      {!entry && (
        <div className="absolute inset-0 flex items-center justify-center gap-2 text-zinc-500 text-[11px] bg-[#09090b]">
          <Loader2 size={13} className="animate-spin" /> Starting…
        </div>
      )}
    </div>
  );
};

/// The pane's left edge: drag it (or use the arrow keys on it) to make the
/// pane wider or narrower. The width is kept on this device.
const Resizer = ({ width, max }: { width: number; max: number }) => {
  const drag = useRef<{ x: number; w: number } | null>(null);
  const clamp = (w: number) => Math.min(max, Math.max(PANE_MIN_WIDTH, w));
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Width of the AI pane"
      aria-valuenow={width}
      aria-valuemin={PANE_MIN_WIDTH}
      aria-valuemax={max}
      tabIndex={0}
      title="Drag to change the width"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { x: e.clientX, w: width };
      }}
      onPointerMove={(e) => {
        if (!drag.current) return;
        setPaneWidth(clamp(drag.current.w + (drag.current.x - e.clientX)));
      }}
      onPointerUp={(e) => {
        if (!drag.current) return;
        setPaneWidth(clamp(drag.current.w + (drag.current.x - e.clientX)), true);
        drag.current = null;
      }}
      onPointerCancel={() => { drag.current = null; }}
      onKeyDown={(e) => {
        if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
        e.preventDefault();
        setPaneWidth(clamp(width + (e.key === "ArrowLeft" ? 24 : -24)), true);
      }}
      className="absolute left-0 top-0 bottom-0 -ml-1 w-2 z-20 cursor-col-resize hover:bg-primary/30 focus-visible:bg-primary/40 outline-none"
    />
  );
};

export default AiSidePane;

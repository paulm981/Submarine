import { useSyncExternalStore } from "react";
import type { Terminal } from "@xterm/xterm";

// ─────────────────────────────────────────────────────────────────────────────
// Terminal dock — where a terminal is on screen.
//
// Each terminal has ONE xterm (TerminalView), drawn into a plain <div>
// ("host") that React doesn't manage, so it can be moved without remounting.
// An AI tab's terminal, which lives in Home, is moved into the AI side pane
// beside a server tab (`claimTerminal`, AiSidePane) and back when the pane
// folds or closes — one place at a time, never a copy. The registry also
// lets the pane find a server tab's terminal, to give it the keyboard back.
// ─────────────────────────────────────────────────────────────────────────────

export interface DockEntry {
  terminalId: string;
  sessionId: string;
  term: Terminal;
  /// The div xterm draws into.
  host: HTMLElement;
  /// Where the host sits when the AI side pane doesn't have it (inside
  /// TerminalView).
  home: HTMLElement;
  /// Called after the host moved, to refit it to its new place.
  moved: (docked: boolean) => void;
}

const entries = new Map<string, DockEntry>();
// terminalId → the place (the AI side pane) that wants it on screen.
const claims = new Map<string, HTMLElement>();
const listeners = new Set<() => void>();
let version = 0;

const changed = () => {
  version++;
  for (const l of listeners) l();
};

const place = (e: DockEntry) => {
  const slot = claims.get(e.terminalId);
  const target = slot ?? e.home;
  if (e.host.parentElement !== target) {
    target.appendChild(e.host);
    e.moved(!!slot);
  }
};

// ── Settling after a move ───────────────────────────────────────────────────
// After a move (into the AI side pane, back to Home, or when it first opens)
// a terminal refits to its new box, redraws, and takes or gives up a GPU
// context. A refit re-wraps the whole scrollback, so for many terminals at
// once that held the window for seconds (#127). They settle a few at a time
// instead, in the order they moved, within FRAME_BUDGET_MS of each frame: the
// window keeps answering while the rest catch up.

const FRAME_BUDGET_MS = 8;
/// Terminal id → what it does once it settles; the latest move's only.
const settling = new Map<string, () => void>();
let settleScheduled = false;

const settleSome = () => {
  settleScheduled = false;
  const start = performance.now();
  for (const [id, job] of settling) {
    settling.delete(id);
    try {
      job();
    } catch {
      /* not laid out yet: its ResizeObserver refits */
    }
    if (performance.now() - start >= FRAME_BUDGET_MS) break;
  }
  if (settling.size > 0 && !settleScheduled) {
    settleScheduled = true;
    requestAnimationFrame(settleSome);
  }
};

/// Runs `job` for `terminalId` in a coming frame, after the terminals that
/// moved before it. A newer move's job replaces one still waiting.
export const settleAfterMove = (terminalId: string, job: () => void) => {
  settling.delete(terminalId);
  settling.set(terminalId, job);
  if (!settleScheduled) {
    settleScheduled = true;
    requestAnimationFrame(settleSome);
  }
};

/// TerminalView registers its terminal once its xterm exists; the returned
/// function unregisters it (on unmount).
export const registerTerminal = (e: DockEntry): (() => void) => {
  entries.set(e.terminalId, e);
  place(e);
  changed();
  return () => {
    if (entries.get(e.terminalId) !== e) return;
    entries.delete(e.terminalId);
    settling.delete(e.terminalId);
    e.host.remove();
    changed();
  };
};

/// The AI side pane puts the terminal in `slot` (now, or whenever it
/// registers); the returned function gives it back to its home.
export const claimTerminal = (terminalId: string, slot: HTMLElement): (() => void) => {
  claims.set(terminalId, slot);
  const e = entries.get(terminalId);
  if (e) place(e);
  return () => {
    if (claims.get(terminalId) !== slot) return;
    claims.delete(terminalId);
    const cur = entries.get(terminalId);
    if (cur) place(cur);
  };
};

export const dockedTerminal = (terminalId: string): DockEntry | undefined => entries.get(terminalId);

/// For TerminalView to say its terminal's look changed (its colors), so the
/// AI side pane repaints around it.
export const terminalStateChanged = () => changed();

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

/// Re-renders the caller whenever a terminal registers, unregisters or
/// changes its look.
export const useDockVersion = (): number => useSyncExternalStore(subscribe, () => version);

// ── App shortcuts inside a terminal ─────────────────────────────────────────

/// Keys the app takes before a terminal sees them (xterm would otherwise
/// turn Ctrl+Shift+K into Ctrl+K for the shell): Ctrl+Shift+K jumps to a
/// session by name, Ctrl+Shift+L opens or folds the AI side pane (AiSidePane;
/// Ctrl+Shift+A is the terminal's Select all), Ctrl+Tab and Ctrl+Shift+Tab go
/// to the next and previous tab. For those two xterm would send the shell Tab
/// and back-tab, which it still gets from Tab and Shift+Tab. The shifted
/// letters are free: the shell only ever sees the plain Ctrl+letter.
export type AppHotkey = "jump" | "aiPane" | "next" | "prev";
// Ctrl+Shift+L is the pane's only while it can do something with it (the
// pane says so as it renders: AI tools on, an AI tab open, not in Home, the
// window wide enough for it). Otherwise the terminal keeps it, and the shell
// gets Ctrl+L (clear the screen) as before.
let aiPaneKeyLive = false;
export const setAiPaneKeyLive = (on: boolean) => { aiPaneKeyLive = on; };
export const appHotkey = (e: KeyboardEvent): AppHotkey | null => {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.code === "Tab") return e.shiftKey ? "prev" : "next";
  if (!e.shiftKey) return null;
  if (e.code === "KeyK") return "jump";
  if (e.code === "KeyL" && aiPaneKeyLive) return "aiPane";
  return null;
};

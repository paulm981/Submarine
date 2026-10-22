import { useSyncExternalStore } from "react";
import type { Terminal } from "@xterm/xterm";

// ─────────────────────────────────────────────────────────────────────────────
// Terminal dock — where a server terminal is on screen.
//
// Each SSH terminal has ONE xterm (TerminalView), drawn into a plain <div>
// ("host") that React doesn't manage, so it can be moved without remounting:
// it sits in its session tab, and while a Wall showing it is on screen it is
// moved into that Wall's tile (`claimTerminal`) and back when the Wall goes
// away. Like MobaXterm's MultiExec, the Wall shows the terminals themselves —
// their whole scrollback, sized to the tile (the shell wraps at the tile's
// width, not the tab's) — rather than copies that start empty. An AI tab's
// terminal, which lives in Home, moves the same way into the AI side pane
// beside a server tab (AiSidePane) — one place at a time, never a copy.
//
// The registry is also how the Wall types to many terminals at once: it
// finds each one here to ask whether it can take input, which cursor-key
// mode its program is in, and to paste into it the way its own paste does.
// ─────────────────────────────────────────────────────────────────────────────

export interface DockEntry {
  terminalId: string;
  sessionId: string;
  term: Terminal;
  /// The div xterm draws into.
  host: HTMLElement;
  /// Where the host sits when no Wall has it (inside TerminalView).
  home: HTMLElement;
  /// Its shell is open and its session connected: input reaches it.
  live: () => boolean;
  /// Paste the way the terminal's own paste does: one Enter per line break,
  /// wrapped as a bracketed paste when the program asked for that. False
  /// when it can't take input.
  paste: (text: string) => boolean;
  /// Called after the host moved, to refit it to its new place.
  moved: (docked: boolean) => void;
}

const entries = new Map<string, DockEntry>();
// terminalId → a number that changes whenever its line may have changed
// under a Wall: its shell opened (again), or something was typed into it.
// A Wall keeps a line typed to all to the terminals whose number is still
// the one it saw after typing the start of it.
const marks = new Map<string, number>();
let lastMark = 0;
/// Typed into, or opened: by the terminal itself, a panel, or a Wall
/// (which reads the new number after it types, terminalMark).
export const markTyped = (...terminalIds: string[]) => {
  for (const id of terminalIds) marks.set(id, ++lastMark);
};
export const terminalMark = (terminalId: string) => marks.get(terminalId) ?? 0;
// terminalId → the Wall tile that wants it on screen.
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

/// TerminalView registers its terminal once its xterm exists; the returned
/// function unregisters it (on unmount).
export const registerTerminal = (e: DockEntry): (() => void) => {
  entries.set(e.terminalId, e);
  place(e);
  changed();
  return () => {
    if (entries.get(e.terminalId) !== e) return;
    entries.delete(e.terminalId);
    e.host.remove();
    changed();
  };
};

/// A Wall tile puts the terminal in `slot` (now, or whenever it registers);
/// the returned function gives it back to its tab.
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

/// For TerminalView to say its terminal's liveness changed (shell opened,
/// session dropped), so a Wall recounts who it is typing to.
export const terminalStateChanged = () => changed();

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

/// Re-renders the caller whenever a terminal registers, unregisters or
/// changes liveness.
export const useDockVersion = (): number => useSyncExternalStore(subscribe, () => version);

// ── App shortcuts inside a terminal ─────────────────────────────────────────

/// Keys the app takes before a terminal sees them (xterm would otherwise
/// turn Ctrl+Shift+B into Ctrl+B for the shell): Ctrl+Shift+B turns
/// broadcast on or off, Ctrl+Shift+P pauses a terminal in a broadcast,
/// Ctrl+Shift+K jumps to a session by name, Ctrl+Shift+L opens or folds the
/// AI side pane (AiSidePane; Ctrl+Shift+A is the terminal's Select all),
/// Ctrl+Tab and Ctrl+Shift+Tab go to the next and previous tab. For those two
/// xterm would send the shell Tab and back-tab, which it still gets from Tab
/// and Shift+Tab. The shifted letters are free: the shell only ever sees the
/// plain Ctrl+letter.
export type AppHotkey = "broadcast" | "pause" | "jump" | "aiPane" | "next" | "prev";
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
  if (e.code === "KeyB") return "broadcast";
  if (e.code === "KeyP") return "pause";
  if (e.code === "KeyK") return "jump";
  if (e.code === "KeyL" && aiPaneKeyLive) return "aiPane";
  return null;
};

// ── Cursor keys per terminal ────────────────────────────────────────────────

const CURSOR_FINALS = "ABCDHF";

/// The Wall's own keyboard terminal never sees a program's output, so its
/// arrow, Home and End keys always come out in normal mode (`ESC [ A`).
/// A program that asked for application cursor keys (vim, less, and some
/// shells' line editors) expects `ESC O A`; this gives each terminal the
/// form its program asked for. Modified keys (`ESC [ 1 ; 5 A`) are the same in both.
export const forCursorMode = (data: string, applicationCursor: boolean): string => {
  if (!applicationCursor || data.length !== 3 || data[0] !== "\x1b" || data[1] !== "[") return data;
  return CURSOR_FINALS.includes(data[2]) ? `\x1bO${data[2]}` : data;
};

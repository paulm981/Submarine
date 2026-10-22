import { useSyncExternalStore } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// The AI side pane (#100): an AI tab's terminal shown beside a server tab.
//
// The AI tab itself lives in Home (HomePane), as a terminal on this computer.
// The pane shows that same terminal, moved the way a Wall borrows a server's
// (src/ui/terminalDock.ts) — never a copy, so the conversation is only ever
// in one place. This is the state the pane, the title bar and Home share:
//   • whether the pane is folded, and its width (remembered on this device);
//   • the view the title bar's button opened it on, where nothing is shared;
//   • which AI tab it shows when there's more than one;
//   • requests to Home: start an AI tool (a server tab's "Claude Code beside
//     this"), and show one of its tabs ("Back to Home").
// ─────────────────────────────────────────────────────────────────────────────

const FOLDED_KEY = "submarine-ai-pane-folded";
const WIDTH_KEY = "submarine-ai-pane-width";
export const PANE_MIN_WIDTH = 300;
const PANE_DEFAULT_WIDTH = 520;

const readFolded = (): boolean => {
  try { return localStorage.getItem(FOLDED_KEY) === "1"; } catch { return false; }
};
const readWidth = (): number => {
  let w = NaN;
  try { w = parseInt(localStorage.getItem(WIDTH_KEY) || "", 10); } catch { /* not available */ }
  return Number.isFinite(w) && w >= PANE_MIN_WIDTH ? w : PANE_DEFAULT_WIDTH;
};

/// An AI tool Home is asked to start. `onStarted` runs once it has started
/// and its AI tab exists (it can then be shared with).
export interface AiLaunch { id: string; shell: string; label: string; onStarted: (tabId: string) => void }

export interface AiPaneState {
  folded: boolean;
  width: number;
  /// The view the title bar's button (or Ctrl+Shift+L) opened the pane on,
  /// where it shows though nothing there is shared. Gone when that view is
  /// left, or the pane is closed there.
  here: string | null;
  /// The AI tab picked in the pane's header.
  chosen: string | null;
  /// An AI tab started from a server tab's + menu, shown before it has
  /// finished starting.
  starting: { id: string; label: string } | null;
  /// Home must be mounted: an AI tool was started before Home was ever shown.
  homeNeeded: boolean;
}

let state: AiPaneState = {
  folded: readFolded(), width: readWidth(), here: null, chosen: null, starting: null, homeNeeded: false,
};
const listeners = new Set<() => void>();
const set = (patch: Partial<AiPaneState>) => {
  state = { ...state, ...patch };
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

export const useAiPane = (): AiPaneState => useSyncExternalStore(subscribe, () => state);
export const aiPaneState = () => state;

export const setPaneFolded = (folded: boolean) => {
  if (state.folded === folded) return;
  set({ folded });
  try { localStorage.setItem(FOLDED_KEY, folded ? "1" : "0"); } catch { /* not kept */ }
};
/// While dragging: on screen only. `keep` saves it for next time.
export const setPaneWidth = (width: number, keep = false) => {
  const w = Math.max(PANE_MIN_WIDTH, Math.round(width));
  if (state.width !== w) set({ width: w });
  if (keep) {
    try { localStorage.setItem(WIDTH_KEY, String(w)); } catch { /* not kept */ }
  }
};
export const setPaneHere = (here: string | null) => {
  if (state.here !== here) set({ here });
};
export const choosePaneAi = (chosen: string) => {
  if (state.chosen !== chosen) set({ chosen });
};
/// The view changed: what was opened on the last one only is closed.
export const leftView = (view: string) => {
  if (state.here !== null && state.here !== view) set({ here: null, starting: null });
};
/// Locking the profile: nothing carries over to the next one.
export const resetAiPane = () => {
  launches = [];
  set({ here: null, chosen: null, starting: null, homeNeeded: false });
};
/// The pane closed where it was opened by itself: an AI tool still starting
/// there is no longer waited for in it.
export const closePaneHere = () => {
  if (state.here !== null || state.starting !== null) set({ here: null, starting: null });
};

// ── Requests to Home ────────────────────────────────────────────────────────

let launches: AiLaunch[] = [];
const launchWatchers = new Set<() => void>();
const newId = () => `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

/// Start an AI tool in a new Home tab; shown in the pane on `view` while it
/// starts. Returns the new tab's id.
export const launchAiTool = (shell: string, label: string, view: string, onStarted: (tabId: string) => void): string => {
  const id = newId();
  launches = [...launches, { id, shell, label, onStarted }];
  set({ homeNeeded: true, here: view, chosen: id, starting: { id, label }, folded: false });
  try { localStorage.setItem(FOLDED_KEY, "0"); } catch { /* not kept */ }
  for (const w of launchWatchers) w();
  return id;
};
/// Home takes the requests waiting for it.
export const takeAiLaunches = (): AiLaunch[] => {
  const out = launches;
  launches = [];
  return out;
};
export const watchAiLaunches = (w: () => void) => {
  launchWatchers.add(w);
  return () => { launchWatchers.delete(w); };
};
/// The AI tab has started (it's in the AI state): the pane goes on to show
/// it as any other.
/// …or never will: its Home tab closed, or it failed to start.
export const aiStartEnded = (id: string) => {
  if (state.starting?.id === id) set({ starting: null });
};
export const aiStarted = (id: string) => {
  if (state.starting?.id === id) set({ starting: null });
};

let homeTabAsk: string | null = null;
const homeTabWatchers = new Set<() => void>();
/// "Back to Home": Home shows this tab.
export const showHomeTab = (tabId: string) => {
  homeTabAsk = tabId;
  for (const w of homeTabWatchers) w();
};
export const takeHomeTabAsk = (): string | null => {
  const id = homeTabAsk;
  homeTabAsk = null;
  return id;
};
export const watchHomeTabAsk = (w: () => void) => {
  homeTabWatchers.add(w);
  return () => { homeTabWatchers.delete(w); };
};

import { useSyncExternalStore } from "react";
import type { Mode } from "./plan";

// ─────────────────────────────────────────────────────────────────────────────
// Bulk push and pull (#159): which dialog to show, asked for from a tab's or
// the session tree's right-click menu. The dialog (BulkDialog.tsx) is always
// mounted, so a run goes on while it's hidden; this is only the request.
// ─────────────────────────────────────────────────────────────────────────────

export interface BulkAsk {
  mode: Mode;
  /** The sessions the menu was about: picked to start with, unless
   *  Broadcast is on with targets of its own. */
  from: string[];
  /** Changes on every ask, so the same ask twice opens it twice. */
  seq: number;
}

let ask: BulkAsk | null = null;
let seq = 0;
const listeners = new Set<() => void>();
const changed = (next: BulkAsk | null) => {
  ask = next;
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

export const openBulk = (mode: Mode, from: string[]) => changed({ mode, from, seq: ++seq });
export const hideBulk = () => { if (ask) changed(null); };
export const useBulkAsk = (): BulkAsk | null => useSyncExternalStore(subscribe, () => ask);

// Whether a run is going, hidden or not, or ended while hidden: the menus
// then offer to show it.
let running = false;
const runningListeners = new Set<() => void>();
export const setBulkRunning = (on: boolean) => {
  if (running === on) return;
  running = on;
  for (const l of runningListeners) l();
};
const subscribeRunning = (l: () => void) => {
  runningListeners.add(l);
  return () => { runningListeners.delete(l); };
};
export const useBulkRunning = (): boolean => useSyncExternalStore(subscribeRunning, () => running);

/** A session the dialog can copy over: SSH, and connected. */
export const canBulk = (s: { protocol?: string } | undefined, status: string | undefined): boolean =>
  !!s && (s.protocol ?? "ssh") === "ssh" && status === "connected";

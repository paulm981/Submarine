// Who Broadcast's typing reaches (#218): all of them, one of them, or all
// but the paused ones.
//
// - **Typing to all** is where it starts: what's typed into one ticked
//   server's terminal goes to every other ticked server.
// - **Typing to one** (`solo`): what's typed goes to the terminal it's typed
//   into, and nowhere else. Typing into another one moves "one" there; only
//   "Type to all" goes back to all, never a click.
// - **Paused** terminals are left out: nothing typed elsewhere reaches them,
//   and what's typed into one of them goes to it alone.
// - **Picked** terminals are for pausing or resuming several at once: Ctrl
//   (⌘) click picks one, Shift-click the run from the last one picked.
// - When Broadcast ends, it's all forgotten (`resetAim`): next time every
//   ticked server gets the keys.
//
// The fork's broadcastWrite asks `reaches` for every key it sends on, so
// this is the one place that says who gets it.
import { useSyncExternalStore } from "react";
import { rangePick } from "../../util/rangePick";

export type Aim = {
  /** The terminal typed to alone; null: typing to all. */
  solo: string | null;
  /** Terminals left out. */
  paused: ReadonlySet<string>;
  /** Terminals picked to pause or resume together, in the order picked. */
  picked: readonly string[];
  /** The last one picked without Shift, where a Shift-click's run starts. */
  anchor: string | null;
  /** What was picked before the current run of Shift-clicks. */
  runBase: readonly string[] | null;
  /** The Wall's terminals in tile order, for Shift-click runs. */
  order: readonly string[];
};

const START: Aim = { solo: null, paused: new Set(), picked: [], anchor: null, runBase: null, order: [] };
let aim: Aim = START;
const listeners = new Set<() => void>();
const set = (next: Partial<Aim>) => {
  aim = { ...aim, ...next };
  for (const l of listeners) l();
};

/** Does what's typed into `from` go on to `to`? */
export const reaches = (a: Aim, from: string, to: string): boolean =>
  a.solo === null && !a.paused.has(from) && !a.paused.has(to);

/** Broadcast's typing into `from`, on its way to `to`: whether it goes on.
 *  Typing to one and typing into another moves "one" to it. */
export const sendsOn = (from: string, to: string): boolean => {
  if (aim.solo !== null && aim.solo !== from) set({ solo: from });
  return reaches(aim, from, to);
};

export const aimNow = (): Aim => aim;

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};
/** Re-renders the caller when who's typed to changes. */
export const useAim = (): Aim => useSyncExternalStore(subscribe, aimNow);

export const typeToOne = (terminalId: string) => { if (aim.solo !== terminalId) set({ solo: terminalId }); };
export const typeToAll = () => { if (aim.solo !== null) set({ solo: null }); };

/** Pause `ids` (or resume them, with `pause` false), and drop the picks. */
export const setPaused = (ids: readonly string[], pause: boolean) => {
  const paused = new Set(aim.paused);
  for (const id of ids) {
    if (pause) paused.add(id);
    else paused.delete(id);
  }
  set({ paused, picked: [], anchor: null, runBase: null });
};
export const togglePaused = (id: string) => setPaused([id], !aim.paused.has(id));

/** A click that picks: Shift for the run from the anchor to `id`, on top of
 *  what was picked before the run; otherwise `id` in or out. */
export const pick = (id: string, shift: boolean) => {
  if (shift && aim.anchor) {
    const base = aim.runBase ?? aim.picked;
    const next = rangePick(aim.order, aim.anchor, id, base);
    if (next) {
      set({ picked: next, runBase: base });
      return;
    }
  }
  set({ picked: aim.picked.includes(id) ? aim.picked.filter((p) => p !== id) : [...aim.picked, id], anchor: id, runBase: null });
};
export const clearPicks = () => { if (aim.picked.length || aim.anchor) set({ picked: [], anchor: null, runBase: null }); };

/** The Wall's tiles changed: the order for runs, and a pick of a terminal no
 *  longer there dropped. */
export const setOrder = (order: readonly string[]) => {
  const same = order.length === aim.order.length && order.every((t, i) => t === aim.order[i]);
  if (same) return;
  const there = new Set(order);
  const picked = aim.picked.filter((p) => there.has(p));
  set({ order, picked: picked.length === aim.picked.length ? aim.picked : picked });
};

/** Broadcast ended: everything back to typing to all, nothing paused. */
export const resetAim = () => { if (aim !== START) { aim = START; for (const l of listeners) l(); } };

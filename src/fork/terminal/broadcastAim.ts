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
// - **A line at a time.** A server that joins partway through a line (ticked,
//   resumed, back from typing to one) waits for the next line, so it never
//   gets the end of a command without its start: `rm -rf /tmp/x` mustn't
//   reach one as `/x`. The line is followed as a shell would (broadcastLine):
//   it ends at Enter, unless the shell would ask for more, or at Ctrl+C. A
//   server that misses a key of the line (paused, not connected) waits too.
// - When Broadcast ends, it's all forgotten (`resetAim`): next time every
//   ticked server gets the keys.
//
// The fork's broadcastWrite asks `reaches` for every key it sends on, so
// this is the one place that says who gets it.
import { useSyncExternalStore } from "react";
import { rangePick } from "../../util/rangePick";
import { lineAfter } from "./broadcastLine";

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
  /** While a line is open: the terminals that have had all of it. Any other
   *  waits for the next line. Null: no line open. */
  line: ReadonlySet<string> | null;
};

const START: Aim = { solo: null, paused: new Set(), picked: [], anchor: null, runBase: null, order: [], line: null };
let aim: Aim = START;
const listeners = new Set<() => void>();
const set = (next: Partial<Aim>) => {
  aim = { ...aim, ...next };
  for (const l of listeners) l();
};

/** Does what's typed into `from` go on to `to`? */
export const reaches = (a: Aim, from: string, to: string): boolean =>
  a.solo === null && !a.paused.has(from) && !a.paused.has(to) && (a.line === null || a.line.has(to));

// The line open, as typed so far (null: none), and the key being sent on:
// one key's writes to its targets come together, in one turn of the page,
// with the same bytes.
let typed: string | null = null;
type Key = { from: string | null; data: number[]; members: ReadonlySet<string> | null; reached: Set<string> };
let key: Key | null = null;

/** Pastes longer than this aren't read for where the line ends: the line is
 *  taken to be open until the next Enter, so a server joining waits. */
export const READ_MOST = 4096;

const decoder = new TextDecoder();
const sameSet = (a: ReadonlySet<string> | null, b: ReadonlySet<string> | null) =>
  a === b || (!!a && !!b && a.size === b.size && [...a].every((x) => b.has(x)));
const setLine = (line: ReadonlySet<string> | null) => { if (!sameSet(aim.line, line)) set({ line }); };

/** The key's writes are all asked for: the line goes on, or ends. Those that
 *  got the key (and the one it was typed into, if it had the line) have the
 *  line now; any other waits. */
const endKey = () => {
  const k = key;
  key = null;
  if (!k) return;
  const text = decoder.decode(new Uint8Array(k.data));
  const was = typed;
  // A bracketed paste ends with an escape, which leaves the line open: the
  // shell holds the text, line breaks and all, until Enter.
  typed = text.length > READ_MOST ? "" : lineAfter(was, text);
  if (typed === null) return setLine(null);
  const members = new Set(k.reached);
  if (k.from !== null && (k.members === null || k.members.has(k.from))) members.add(k.from);
  setLine(members);
};

/** `from` was typed into on its own (typing to one, or it's paused): the
 *  line it has isn't the others' any more. */
const leaveLine = (from: string) => {
  if (aim.line?.has(from)) setLine(new Set([...aim.line].filter((t) => t !== from)));
};

/** Broadcast's typing into `from`, `data`, on its way to `to`: whether it
 *  goes on. Typing to one and typing into another moves "one" to it. */
export const sendsOn = (from: string, to: string, data: number[]): boolean => {
  if (aim.solo !== null && aim.solo !== from) set({ solo: from });
  if (aim.solo !== null || aim.paused.has(from)) {
    leaveLine(from);
    return false;
  }
  if (!key || key.from !== from || key.data !== data) {
    endKey();
    key = { from, data, members: aim.line, reached: new Set() };
    const mine = key;
    queueMicrotask(() => { if (key === mine) endKey(); });
  }
  const ok = reaches(aim, from, to);
  if (ok) key.reached.add(to);
  return ok;
};

/** Who a Send to all reaches now, of `ids` (Broadcast's terminals that are
 *  connected): not paused, and not waiting for the next line. */
export const sendTargets = (a: Aim, ids: readonly string[]) => {
  const paused = ids.filter((t) => a.paused.has(t));
  const waiting = ids.filter((t) => !a.paused.has(t) && a.line !== null && !a.line.has(t));
  const to = ids.filter((t) => !a.paused.has(t) && (a.line === null || a.line.has(t)));
  return { to, paused: paused.length, waiting: waiting.length };
};

/** `data` was sent to `ids` all at once (Send to all): the line goes on, or
 *  ends, with them. */
export const sentToAll = (ids: readonly string[], data: number[]) => {
  endKey();
  key = { from: null, data, members: aim.line, reached: new Set(ids) };
  endKey();
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

/** Broadcast ended: everything back to typing to all, nothing paused, no
 *  line open. */
export const resetAim = () => {
  typed = null;
  key = null;
  if (aim !== START) {
    aim = START;
    for (const l of listeners) l();
  }
};

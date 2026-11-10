// Broadcast's typing, one call for every target (#127, #205).
//
// The original project's Broadcast writes what's typed to each target with a
// call of its own: 36 servers, 36 calls across the bridge for every key. On
// Windows each call is a request through the webview, so typing lagged.
// TerminalView's fan-out calls broadcastWrite in place of
// `invoke('write_terminal_data', …)`, with the same arguments: the writes of
// one keystroke (the same bytes, in the same turn of the page) go out as one
// `write_terminals_data`, in the order they were asked for.
//
// - **A slow server holds up only itself.** The backend never waits on a
//   terminal: one whose queue is full (a big paste still going out on a slow
//   link) is named in its answer, and from then on that terminal gets its
//   keys here, one `write_terminal_data` at a time and in order, until it has
//   caught up. The others carry on in one call per key.
// - **Any size.** What's typed is cut into pieces the backend takes
//   (MAX_DATA), in order, so a big paste reaches every server.
// - **A failed call** (the command gone after a sync, say) sends that key to
//   each target on its own, as the original project does.
// - **Who it reaches** (typing to one, paused ones) is broadcastAim's to
//   say: TerminalView names the terminal typed into, `from`.
import { invoke } from "@tauri-apps/api/core";
import { sendsOn, sentToAll } from "./broadcastAim";

/** The most targets in one call, and bytes in one write, as the backend
 *  allows (multiexec.rs: MAX_TARGETS, MAX_DATA). */
export const MAX_TARGETS = 512;
export const MAX_DATA = 1024 * 1024;
/** The most bytes waiting for one slow terminal. A terminal this far behind
 *  isn't taking what it's sent: more for it is dropped until it catches up,
 *  so it can't fill the window's memory. */
export const MAX_BEHIND = 16 * 1024 * 1024;

type Write = { terminalId: string; data: number[] };
type Call = { terminalIds: string[]; data: number[] };

/** The calls for what's waiting: one per run of writes with the same bytes,
 *  each at most MAX_TARGETS terminals, and each run's bytes in pieces of at
 *  most MAX_DATA, in order. Exported for the tests. */
export const callsFor = (writes: Write[]): Call[] => {
  const runs: Call[] = [];
  for (const w of writes) {
    const last = runs[runs.length - 1];
    if (last && last.data === w.data && last.terminalIds.length < MAX_TARGETS) last.terminalIds.push(w.terminalId);
    else runs.push({ terminalIds: [w.terminalId], data: w.data });
  }
  return runs.flatMap((run) => {
    if (run.data.length <= MAX_DATA) return [run];
    const pieces: Call[] = [];
    for (let at = 0; at < run.data.length; at += MAX_DATA) pieces.push({ terminalIds: run.terminalIds, data: run.data.slice(at, at + MAX_DATA) });
    return pieces;
  });
};

/** Terminals that are behind: their own writes, one after another, and the
 *  bytes still to go. */
const behind = new Map<string, { chain: Promise<void>; bytes: number }>();

/** `data` to one terminal, after what's still on its way to it. */
const alone = (terminalId: string, data: number[]) => {
  const mine = behind.get(terminalId) ?? { chain: Promise.resolve(), bytes: 0 };
  if (mine.bytes + data.length > MAX_BEHIND) return;
  behind.set(terminalId, mine);
  mine.bytes += data.length;
  mine.chain = mine.chain
    .then(() => invoke("write_terminal_data", { terminalId, data }))
    .then(
      () => {},
      () => {},
    )
    .then(() => {
      mine.bytes -= data.length;
      if (mine.bytes === 0 && behind.get(terminalId) === mine) behind.delete(terminalId);
    });
};

/** One call: to the terminals that aren't behind together, and to each one
 *  that is, or turns out to be (its queue full), on its own. */
const send = async ({ terminalIds, data }: Call): Promise<void> => {
  const together = terminalIds.filter((id) => !behind.has(id));
  for (const id of terminalIds) if (behind.has(id)) alone(id, data);
  if (!together.length) return;
  let full: string[];
  try {
    full = (await invoke<string[] | null>("write_terminals_data", { terminalIds: together, data })) ?? [];
  } catch {
    full = together;
  }
  for (const id of together) if (full.includes(id)) alone(id, data);
};

let queued = false;
let writes: Write[] = [];
let flushed: Promise<void> = Promise.resolve();

const flush = () => {
  queued = false;
  const now = callsFor(writes);
  writes = [];
  // Each call after the one before it, so a key never overtakes the last.
  // None of them waits on a slow server, so this never backs up behind one.
  flushed = now.reduce((prev, call) => prev.then(() => send(call)), flushed);
};

/** A Broadcast write, of what was typed into `from`, to one terminal: sent
 *  with the others of this turn, unless it's not to reach that one. */
export const broadcastWrite = (from: string, { terminalId, data }: Write): Promise<void> => {
  if (!sendsOn(from, terminalId, data)) return Promise.resolve();
  writes.push({ terminalId, data });
  if (!queued) {
    queued = true;
    queueMicrotask(flush);
  }
  return Promise.resolve();
};

/** Send to all: `data` to each of `terminalIds`, as one write, whoever's
 *  typed to; the caller has picked who (broadcastAim's sendTargets). */
export const broadcastSend = (terminalIds: readonly string[], data: number[]) => {
  if (!terminalIds.length) return;
  for (const terminalId of terminalIds) writes.push({ terminalId, data });
  if (!queued) {
    queued = true;
    queueMicrotask(flush);
  }
  sentToAll(terminalIds, data);
};

/** Done once everything asked for so far has gone, or is on its way to a
 *  terminal that's behind. For the tests. */
export const sent = (): Promise<void> => new Promise((done) => queueMicrotask(() => void flushed.then(done)));

/** Done once every terminal that's behind has caught up. For the tests. */
export const caughtUp = async (): Promise<void> => {
  await sent();
  while (behind.size) await Promise.all([...behind.values()].map((b) => b.chain));
};

// Broadcast's typing, one call for every target (#127).
//
// The original project's Broadcast writes what's typed to each target with a
// call of its own: 36 servers, 36 calls across the bridge for every key. On
// Windows each call is a request through the webview, so typing lagged.
// TerminalView's fan-out calls broadcastWrite in place of
// `invoke('write_terminal_data', …)`, with the same arguments: the writes of
// one keystroke (the same bytes, in the same turn of the page) go out as one
// `write_terminals_data`, in the order they were asked for.
import { invoke } from "@tauri-apps/api/core";

/** The most targets in one call, as the backend allows (multiexec.rs). */
export const MAX_TARGETS = 512;

let queued = false;

/** The calls for what's waiting: one per run of writes with the same bytes,
 *  each at most MAX_TARGETS terminals. Exported for the tests. */
export const callsFor = (writes: { terminalId: string; data: number[] }[]): { terminalIds: string[]; data: number[] }[] => {
  const calls: { terminalIds: string[]; data: number[] }[] = [];
  for (const w of writes) {
    const last = calls[calls.length - 1];
    if (last && last.data === w.data && last.terminalIds.length < MAX_TARGETS) last.terminalIds.push(w.terminalId);
    else calls.push({ terminalIds: [w.terminalId], data: w.data });
  }
  return calls;
};

let writes: { terminalId: string; data: number[] }[] = [];
let flushed: Promise<void> = Promise.resolve();

const flush = () => {
  queued = false;
  const now = callsFor(writes);
  writes = [];
  // Each call after the one before it, so a key never overtakes the last.
  flushed = now.reduce(
    (prev, call) => prev.then(() => invoke("write_terminals_data", call).then(() => {}, () => {})),
    flushed,
  );
};

/** A Broadcast write to one terminal: sent with the others of this turn. */
export const broadcastWrite = ({ terminalId, data }: { terminalId: string; data: number[] }): Promise<void> => {
  writes.push({ terminalId, data });
  if (!queued) {
    queued = true;
    queueMicrotask(flush);
  }
  return Promise.resolve();
};

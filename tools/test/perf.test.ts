// Speed checks: what keeps typing and output from getting slower
// without anyone noticing.
//
// Most of them count work, not time: calls across the bridge for a key,
// timers on a key's way, passes over a batch of output, bytes handed to
// xterm. A count is the same on every machine, so these can't fail because
// a shared CI machine was busy. Where a count stands for today's work (the
// BUDGET numbers), a change that makes it more has to raise the number
// here, on purpose.
//
// The two that do look at the clock have budgets several times under
// today's speed and take the best of a few tries: they catch a change that
// makes highlighting many times slower, not one that makes it 20% slower.
// For that, run `node tools/screenshots/perf-io.mjs` before and after
// (tools/screenshots/README.md).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { batches, corpus } from "../screenshots/corpus.mjs";

type Call = { cmd: string; ids: string[]; bytes: number };
const calls: Call[] = [];
/** Calls the backend hasn't answered yet, when it's told to take its time. */
let slow = false;
const waiting: (() => void)[] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args: { terminalIds?: string[]; terminalId?: string; data: number[] }) => {
    calls.push({ cmd, ids: args.terminalIds ? [...args.terminalIds] : [args.terminalId!], bytes: args.data.length });
    if (slow) return new Promise<null>((done) => waiting.push(() => done(null)));
    return Promise.resolve(cmd === "write_terminals_data" ? [] : null);
  },
}));

const { writeTerminal } = await import("../../src/util/terminalWrite");
const { broadcastWrite } = await import("../../src/fork/terminal/broadcast");
const { resetAim } = await import("../../src/fork/terminal/broadcastAim");
const { buildMatcher, HighlightStream } = await import("../../src/ui/highlight");

const key = (c: string) => Array.from(new TextEncoder().encode(c));
/** The page's work queue run dry, without a timer firing. */
const turns = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };

beforeEach(() => {
  calls.length = 0;
  waiting.length = 0;
  slow = false;
  resetAim();
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("a typed key's way to the backend", () => {
  it("goes at once: one call, in the same turn, behind no timer", () => {
    writeTerminal("speed-1", key("k"));
    // Before anything else runs: nothing batches or delays a key.
    expect(calls).toEqual([{ cmd: "write_terminal_data", ids: ["speed-1"], bytes: 1 }]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("takes one call a key when the backend keeps up, and keys typed behind a slow call go together", async () => {
    for (const c of "echo") {
      writeTerminal("speed-2", key(c));
      await turns();
    }
    expect(calls.length).toBe(4);
    calls.length = 0;
    // A call that takes its time: the 39 keys typed meanwhile are one more call, in order.
    slow = true;
    for (const c of "echo hello from submarine on every server") writeTerminal("speed-2", key(c));
    expect(calls.length).toBe(1);
    waiting.shift()!();
    await turns();
    expect(calls.map((c) => c.bytes)).toEqual([1, 40]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("is one call for all of Broadcast's servers, in the same turn of the page, behind no timer", async () => {
    const ids = Array.from({ length: 36 }, (_, i) => `speed-b${i}`);
    for (const c of "echo hello from submarine on every server") {
      const data = key(c);
      for (const terminalId of ids) void broadcastWrite("source", { terminalId, data });
      // No timer has run: the call left with the page's own work queue.
      await turns();
      expect(vi.getTimerCount()).toBe(0);
    }
    expect(calls.length).toBe(41);
    expect(calls.every((c) => c.cmd === "write_terminals_data" && c.ids.length === 36 && c.bytes === 1)).toBe(true);
  });
});

// ── Highlighting ─────────────────────────────────────────────────────────────

/** Today's work on 1 MiB of each kind of output (corpus.mjs) with the
 *  standard set, in the backend's 256 KiB batches, with a tenth to spare:
 *  runs of the regex engine, and the bytes xterm is handed for each byte
 *  that came (what highlighting adds is color codes for xterm to parse).
 *  A change that needs more raises these on purpose. */
const BUDGET = {
  plain: { execs: 31_600, grows: 1.001 },
  log: { execs: 63_000, grows: 1.35 },
  sgr: { execs: 232_000, grows: 1.001 },
  ls: { execs: 122_700, grows: 1.17 },
  yes: { execs: 769_100, grows: 1.001 },
} as const;

/** `data` through a new stream with the standard set (and `rules` of the
 *  user's own), every batch highlighted in full: the clock the stream reads
 *  is moved on between batches, so the flood budget never cuts in. */
const highlight = (data: Uint8Array, rules: Parameters<typeof buildMatcher>[1] = []) => {
  const matcher = buildMatcher("standard", rules)!;
  let finds = 0;
  const find = matcher.find.bind(matcher);
  matcher.find = (text, edges) => {
    finds++;
    return find(text, edges);
  };
  const stream = new HighlightStream(() => {});
  stream.setMatcher(matcher, false);
  const exec = RegExp.prototype.exec;
  let execs = 0;
  RegExp.prototype.exec = function (this: RegExp, text: string) {
    execs++;
    return exec.call(this, text);
  };
  let out = 0;
  const pieces = batches(data);
  try {
    for (const b of pieces) {
      vi.advanceTimersByTime(5000);
      out += stream.push(b).length;
    }
  } finally {
    RegExp.prototype.exec = exec;
    stream.dispose();
  }
  return { finds, execs, grows: out / data.length, batches: pieces.length };
};

describe("highlighting a batch of output", () => {
  beforeEach(() => {
    vi.stubGlobal("window", globalThis);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("matches a batch in one go, however many lines it has", () => {
    for (const kind of ["plain", "log", "sgr", "ls", "yes"] as const) {
      const r = highlight(corpus(kind, 1 << 20));
      // Twice: the batch, and the end of it that waited for the rest of its line.
      expect(r.finds, kind).toBeLessThanOrEqual(2 * r.batches);
    }
  });

  it("does no more work on each kind of output than its budget", () => {
    const got: Record<string, { execs: number; grows: number }> = {};
    for (const kind of ["plain", "log", "sgr", "ls", "yes"] as const) {
      const r = highlight(corpus(kind, 1 << 20));
      got[kind] = { execs: r.execs, grows: Math.round(r.grows * 1000) / 1000 };
    }
    // Shown when a budget is passed: what to weigh, and what to raise it to.
    const said = JSON.stringify(got);
    for (const kind of ["plain", "log", "sgr", "ls", "yes"] as const) {
      expect(got[kind].execs, `${kind}: runs of the regex engine. Now: ${said}`).toBeLessThanOrEqual(BUDGET[kind].execs);
      expect(got[kind].grows, `${kind}: bytes to xterm for each byte of output. Now: ${said}`).toBeLessThanOrEqual(BUDGET[kind].grows);
    }
  });

  it("never holds back what's typed: a key's echo goes to xterm in the same call, behind no timer", () => {
    const stream = new HighlightStream(() => {});
    stream.setMatcher(buildMatcher("standard", [])!, false);
    for (const c of "echo ok") {
      expect(stream.push(new TextEncoder().encode(c))).toBe(c);
      expect(vi.getTimerCount()).toBe(0);
    }
    // A prompt after a command's short output isn't held either.
    expect(stream.push(new TextEncoder().encode("\r\ntotal 0\r\n$ "))).toContain("$ ");
    expect(vi.getTimerCount()).toBe(0);
    stream.dispose();
  });
});

describe("highlighting against the clock", () => {
  beforeEach(() => {
    vi.useRealTimers();
    vi.stubGlobal("window", globalThis);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /** Megabytes a second through the standard set, the best of `tries`. */
  const speed = (data: Uint8Array, tries: number) => {
    let best = Infinity;
    for (let i = 0; i < tries; i++) {
      const stream = new HighlightStream(() => {});
      stream.setMatcher(buildMatcher("standard", [])!, false);
      const t0 = performance.now();
      for (const b of batches(data)) stream.push(b);
      best = Math.min(best, performance.now() - t0);
      stream.dispose();
    }
    return data.length / 1048576 / (best / 1000);
  };

  // About 25 MB/s today on a slow machine; xterm itself draws 5 to 15.
  it("highlights log lines at 3 MB/s or more", () => {
    expect(speed(corpus("log", 2 << 20), 5)).toBeGreaterThan(3);
  });

  // About 2 µs today.
  it("takes under 0.1 ms for a key's echo", () => {
    const stream = new HighlightStream(() => {});
    stream.setMatcher(buildMatcher("standard", [])!, false);
    const k = new TextEncoder().encode("k");
    let best = Infinity;
    for (let i = 0; i < 5; i++) {
      const t0 = performance.now();
      for (let n = 0; n < 1000; n++) stream.push(k);
      best = Math.min(best, (performance.now() - t0) / 1000);
    }
    stream.dispose();
    expect(best).toBeLessThan(0.1);
  });
});

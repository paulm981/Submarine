// Broadcast's sending (#127, #205): what goes to the backend, in what order,
// when a server is slow or a call fails. The calls themselves are stood in
// for; what broadcast.ts does with their answers is what's tested.

import { beforeEach, describe, expect, it, vi } from "vitest";

/** A call, and the terminals it reached: not those the backend found full, nor any when it failed. */
type Sent = { cmd: string; ids: string[]; data: number[]; reached: string[] };
const calls: Sent[] = [];
/** What the next write_terminals_data answers: the ids it found full, or an error. */
let answer: (ids: string[]) => string[] | Error = () => [];
/** write_terminal_data calls held until let go, to stand for a slow server. */
let holdAlone = false;
const held: (() => void)[] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: { terminalIds?: string[]; terminalId?: string; data: number[] }) => {
    if (cmd === "write_terminals_data") {
      const ids = [...args.terminalIds!];
      const a = answer(ids);
      calls.push({ cmd, ids, data: args.data, reached: a instanceof Error ? [] : ids.filter((id) => !a.includes(id)) });
      if (a instanceof Error) throw a;
      return a;
    }
    calls.push({ cmd, ids: [args.terminalId!], data: args.data, reached: [args.terminalId!] });
    if (holdAlone) await new Promise<void>((go) => held.push(go));
    return null;
  },
}));

const { broadcastWrite, callsFor, caughtUp, MAX_BEHIND, MAX_DATA, MAX_TARGETS, sent } = await import("../../src/fork/terminal/broadcast");

/** One keystroke's writes, as TerminalView's fan-out makes them: the same bytes to each target. */
const type = (ids: string[], text: string) => {
  const data = Array.from(new TextEncoder().encode(text));
  for (const terminalId of ids) void broadcastWrite({ terminalId, data });
};
const text = (s: Sent) => new TextDecoder().decode(new Uint8Array(s.data));
/** What each terminal got, in order, by whichever call. */
const got = () => {
  const by = new Map<string, string>();
  for (const c of calls) for (const id of c.reached) by.set(id, (by.get(id) ?? "") + text(c));
  return Object.fromEntries(by);
};

beforeEach(async () => {
  holdAlone = false;
  while (held.length) held.shift()!();
  await caughtUp();
  calls.length = 0;
  answer = () => [];
});

describe("Broadcast's typing (#127, #205)", () => {
  it("sends each key to every server in one call, keys in order", async () => {
    for (const k of "ls -la\n") type(["a", "b", "c"], k);
    await sent();
    expect(calls.every((c) => c.cmd === "write_terminals_data" && c.ids.join() === "a,b,c")).toBe(true);
    expect(calls.map(text).join("")).toBe("ls -la\n");
  });

  it("sends a server whose queue is full its keys on its own, in order, while the others carry on", async () => {
    holdAlone = true;
    // Full for the first key only: a slot frees up after, and a key sent
    // together then would overtake the one still on its way.
    let first = true;
    answer = (ids) => {
      const full = first && ids.includes("slow") ? ["slow"] : [];
      first = false;
      return full;
    };
    type(["a", "slow", "b"], "x");
    await sent();
    // The rest of the keys while `slow` still hasn't taken the first.
    for (const k of "yz") type(["a", "slow", "b"], k);
    await sent();
    const together = calls.filter((c) => c.cmd === "write_terminals_data");
    expect(together.map((c) => c.ids.join())).toEqual(["a,slow,b", "a,b", "a,b"]);
    // `slow` gets its first key on its own; the rest wait behind it, in order.
    expect(calls.filter((c) => c.cmd === "write_terminal_data").map(text)).toEqual(["x"]);
    while (held.length) {
      held.shift()!();
      await new Promise((r) => setTimeout(r, 0));
    }
    await caughtUp();
    expect(got()).toEqual({ a: "xyz", slow: "xyz", b: "xyz" });
    // Caught up: together again.
    calls.length = 0;
    answer = () => [];
    type(["a", "slow", "b"], "!");
    await sent();
    expect(calls.map((c) => [c.cmd, c.ids.join(), text(c)])).toEqual([["write_terminals_data", "a,slow,b", "!"]]);
  });

  it("sends each server the key on its own when the call fails, and carries on", async () => {
    answer = () => new Error("command write_terminals_data not found");
    type(["a", "b"], "q");
    await caughtUp();
    expect(got()).toEqual({ a: "q", b: "q" });
    answer = () => [];
    calls.length = 0;
    type(["a", "b"], "r");
    await sent();
    expect(calls.map((c) => c.ids.join())).toEqual(["a,b"]);
  });

  it("sends a paste bigger than the backend takes in pieces, in order, to every server", async () => {
    const big = "0123456789".repeat(Math.ceil((2 * MAX_DATA + 5) / 10));
    type(["a", "b"], big);
    await sent();
    expect(calls.length).toBe(3);
    expect(calls.every((c) => c.data.length <= MAX_DATA && c.ids.join() === "a,b")).toBe(true);
    expect(got()).toEqual({ a: big, b: big });
  });

  it("drops what a server that isn't taking anything is sent past a bound", async () => {
    holdAlone = true;
    answer = (ids) => (ids.includes("stuck") ? ["stuck"] : []);
    const chunk = "x".repeat(MAX_DATA);
    for (let i = 0; i < MAX_BEHIND / MAX_DATA + 4; i++) type(["a", "stuck"], `${chunk}`);
    await sent();
    const waiting = calls.filter((c) => c.cmd === "write_terminal_data");
    // One on its way; the rest wait behind it, at most MAX_BEHIND in all.
    expect(waiting.length).toBe(1);
    const queued = calls.filter((c) => c.cmd === "write_terminals_data" && c.ids.includes("stuck")).length;
    expect(queued).toBe(1);
    holdAlone = false;
    while (held.length) held.shift()!();
    await caughtUp();
    expect(got().stuck.length).toBe(MAX_BEHIND);
    expect(got().a.length).toBe((MAX_BEHIND / MAX_DATA + 4) * MAX_DATA);
  });
});

describe("Broadcast's calls (#127)", () => {
  it("makes one call per run of the same bytes, in order", () => {
    const key = [108];
    const next = [115];
    const writes = ["a", "b", "c"].map((terminalId) => ({ terminalId, data: key }));
    expect(callsFor(writes)).toEqual([{ terminalIds: ["a", "b", "c"], data: key }]);
    const two = callsFor([...writes, { terminalId: "a", data: next }, { terminalId: "b", data: next }]);
    expect(two.map((c) => c.terminalIds)).toEqual([["a", "b", "c"], ["a", "b"]]);
    expect(two[1].data).toBe(next);
  });

  it("splits past the backend's limit of targets", () => {
    const key = [1];
    const many = Array.from({ length: MAX_TARGETS + 3 }, (_, i) => ({ terminalId: `t${i}`, data: key }));
    expect(callsFor(many).map((c) => c.terminalIds.length)).toEqual([MAX_TARGETS, 3]);
  });

  it("has the backend's limits", () => {
    const rust = Object.values(import.meta.glob<string>("/src-tauri/src/multiexec.rs", { query: "?raw", import: "default", eager: true }))[0];
    expect(rust).toContain(`pub(crate) static MAX_TARGETS: usize = ${MAX_TARGETS};`);
    expect(rust).toContain("pub(crate) static MAX_DATA: usize = 1024 * 1024;");
    expect(MAX_DATA).toBe(1024 * 1024);
  });
});

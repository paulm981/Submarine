// Who Broadcast's typing reaches (#218): all, one, or all but the paused.

import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: { ids: string[]; data: number[] }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: { terminalIds?: string[]; terminalId?: string; data: number[] }) => {
    calls.push({ ids: cmd === "write_terminals_data" ? args.terminalIds! : [args.terminalId!], data: args.data });
    return null;
  },
}));

const aimMod = await import("../../src/fork/terminal/broadcastAim");
const { aimNow, pick, reaches, resetAim, setOrder, setPaused, togglePaused, typeToAll, typeToOne, clearPicks } = aimMod;
const { broadcastWrite, sent } = await import("../../src/fork/terminal/broadcast");

/** One key (or paste) typed into `from`, as TerminalView's fan-out sends it
 *  on to the other ticked terminals (`all`); the terminals that got it. */
const typeInto = async (from: string, all: string[], text = "a") => {
  calls.length = 0;
  const data = Array.from(new TextEncoder().encode(text));
  for (const to of all) if (to !== from) void broadcastWrite(from, { terminalId: to, data });
  await sent();
  return calls.flatMap((c) => c.ids).sort();
};

const ALL = ["a", "b", "c", "d"];

beforeEach(() => {
  resetAim();
  setOrder(ALL);
});

describe("who Broadcast's typing reaches", () => {
  it("starts with everyone: what's typed into one goes to all the others", async () => {
    expect(await typeInto("a", ALL)).toEqual(["b", "c", "d"]);
  });

  it("typing to one sends nothing on, from that one or any other", async () => {
    typeToOne("b");
    expect(await typeInto("b", ALL)).toEqual([]);
    expect(await typeInto("a", ALL)).toEqual([]);
  });

  it("typing into another while typing to one moves 'one' there, and never back to all by itself", async () => {
    typeToOne("b");
    await typeInto("c", ALL);
    expect(aimNow().solo).toBe("c");
    expect(await typeInto("d", ALL)).toEqual([]);
    expect(aimNow().solo).toBe("d");
    typeToAll();
    expect(await typeInto("d", ALL)).toEqual(["a", "b", "c"]);
  });

  it("leaves a paused one out, and what's typed into a paused one goes to it alone", async () => {
    togglePaused("c");
    expect(await typeInto("a", ALL)).toEqual(["b", "d"]);
    expect(await typeInto("c", ALL)).toEqual([]);
    togglePaused("c");
    // Resumed partway through the line: it joins at the next.
    expect(await typeInto("a", ALL, "\r")).toEqual(["b", "d"]);
    expect(await typeInto("a", ALL)).toEqual(["b", "c", "d"]);
  });

  it("forgets it all when Broadcast ends", async () => {
    typeToOne("a");
    setPaused(["b", "c"], true);
    resetAim();
    expect(aimNow().solo).toBeNull();
    expect(aimNow().paused.size).toBe(0);
    expect(await typeInto("a", ALL)).toEqual(["b", "c", "d"]);
  });

  it("is the same rule written out", () => {
    const a = { ...aimNow(), paused: new Set(["x"]) };
    expect(reaches(a, "y", "z")).toBe(true);
    expect(reaches(a, "x", "z")).toBe(false);
    expect(reaches(a, "y", "x")).toBe(false);
    expect(reaches({ ...a, solo: "y" }, "y", "z")).toBe(false);
  });
});

describe("picking several to pause or resume", () => {
  it("Ctrl-click picks and unpicks one at a time", () => {
    pick("a", false);
    pick("c", false);
    expect(aimNow().picked).toEqual(["a", "c"]);
    pick("a", false);
    expect(aimNow().picked).toEqual(["c"]);
  });

  it("Shift-click picks the run from the last one Ctrl-clicked, growing and shrinking it, keeping what was picked before", () => {
    pick("d", false);
    pick("a", false);
    pick("c", true);
    expect([...aimNow().picked].sort()).toEqual(["a", "b", "c", "d"]);
    pick("b", true);
    expect([...aimNow().picked].sort()).toEqual(["a", "b", "d"]);
  });

  it("Shift-click with nothing picked yet picks just that one", () => {
    pick("b", true);
    expect(aimNow().picked).toEqual(["b"]);
  });

  it("pausing or resuming the picked ones drops the picks", () => {
    pick("a", false);
    pick("b", false);
    setPaused(aimNow().picked, true);
    expect([...aimNow().paused].sort()).toEqual(["a", "b"]);
    expect(aimNow().picked).toEqual([]);
    setPaused(["a"], false);
    expect([...aimNow().paused]).toEqual(["b"]);
  });

  it("drops a pick whose tile left the Wall", () => {
    pick("a", false);
    pick("d", false);
    setOrder(["a", "b", "c"]);
    expect(aimNow().picked).toEqual(["a"]);
    clearPicks();
    expect(aimNow().picked).toEqual([]);
  });
});

describe("a line at a time", () => {
  it("a server that joins partway through a line waits for the next one", async () => {
    // b, c and d are ticked; "rm -rf /tmp/x" is half typed when e is ticked.
    const before = ["a", "b", "c", "d"];
    const after = [...before, "e"];
    for (const ch of "rm -rf /tm") await typeInto("a", before, ch);
    expect(aimNow().line).toEqual(new Set(before));
    expect(await typeInto("a", after, "p")).toEqual(["b", "c", "d"]);
    expect(await typeInto("a", after, "/x\r")).toEqual(["b", "c", "d"]);
    expect(aimNow().line).toBeNull();
    expect(await typeInto("a", after, "l")).toEqual(["b", "c", "d", "e"]);
  });

  it("a line the shell asks more of goes on past Enter", async () => {
    await typeInto("a", ["a", "b"], "for f in *; do\r");
    expect(aimNow().line).not.toBeNull();
    expect(await typeInto("a", ["a", "b", "c"], "echo $f\r")).toEqual(["b"]);
    expect(await typeInto("a", ["a", "b", "c"], "done\r")).toEqual(["b"]);
    expect(aimNow().line).toBeNull();
    expect(await typeInto("a", ["a", "b", "c"], "x")).toEqual(["b", "c"]);
  });

  it("Up opens a line: one joining after it waits, rather than get only what's typed onto the command it brought back", async () => {
    await typeInto("a", ["a", "b"], "\x1b[A");
    expect(await typeInto("a", ["a", "b", "c"], "x\r")).toEqual(["b"]);
    expect(await typeInto("a", ["a", "b", "c"], "y")).toEqual(["b", "c"]);
  });

  it("Ctrl+C ends a line, and a waiting one doesn't get it", async () => {
    await typeInto("a", ["a", "b"], "sleep 9");
    expect(await typeInto("a", ["a", "b", "c"], "\x03")).toEqual(["b"]);
    expect(aimNow().line).toBeNull();
  });

  it("one that misses a key of the line (not connected, so not sent to) waits for the next", async () => {
    await typeInto("a", ALL, "ec");
    expect(await typeInto("a", ["a", "b", "c"], "h")).toEqual(["b", "c"]);
    expect(await typeInto("a", ALL, "o")).toEqual(["b", "c"]);
  });

  it("typing into one that's waiting goes on to those with the line, and it still waits", async () => {
    await typeInto("a", ["a", "b"], "ls");
    expect(await typeInto("c", ["a", "b", "c"], " -l")).toEqual(["a", "b"]);
    expect(aimNow().line?.has("c")).toBe(false);
    expect(await typeInto("a", ["a", "b", "c"], "\r")).toEqual(["b"]);
  });

  it("typing to one with a line open: that one waits for the next line once it's back with all", async () => {
    await typeInto("a", ALL, "ls ");
    typeToOne("b");
    await typeInto("b", ALL, "-la");
    typeToAll();
    expect(await typeInto("a", ALL, "/\r")).toEqual(["c", "d"]);
    expect(await typeInto("a", ALL, "x")).toEqual(["b", "c", "d"]);
  });

  it("a bracketed paste holds the line open, line breaks and all, until Enter", async () => {
    await typeInto("a", ["a", "b"], "\x1b[200~echo 1\recho 2\r\x1b[201~");
    expect(aimNow().line).not.toBeNull();
    expect(await typeInto("a", ["a", "b", "c"], "\r")).toEqual(["b"]);
    expect(aimNow().line).toBeNull();
  });

  it("a paste too long to read is taken as an open line, so one joining waits for the next Enter", async () => {
    await typeInto("a", ["a", "b"], "x".repeat(aimMod.READ_MOST) + "\r");
    expect(aimNow().line).not.toBeNull();
    expect(await typeInto("a", ["a", "b", "c"], "\r")).toEqual(["b"]);
    expect(aimNow().line).toBeNull();
  });

  it("tells the page only when who has the line changes, not on every key", async () => {
    await typeInto("a", ALL, "e");
    const was = aimNow();
    await typeInto("a", ALL, "c");
    await typeInto("a", ALL, "h");
    expect(aimNow()).toBe(was);
  });

  it("forgets the line when Broadcast ends", async () => {
    await typeInto("a", ["a", "b"], "half");
    resetAim();
    setOrder(ALL);
    expect(await typeInto("a", ALL, "x")).toEqual(["b", "c", "d"]);
  });
});

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

/** One key typed into `from`, as TerminalView's fan-out sends it on to the
 *  other ticked terminals; the terminals that got it. */
const typeInto = async (from: string, all: string[]) => {
  calls.length = 0;
  const data = [0x61];
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

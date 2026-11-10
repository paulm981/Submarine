// Broadcast on one screen (#218): its servers' terminals on the Wall, and
// ticking is turning it on.

import { describe, expect, it, vi } from "vitest";
import { broadcastSwitch, syncBroadcastTiles, type WallTile } from "../../src/fork/desktop/broadcastWall";

const tile = (sessionId: string, terminalId: string): WallTile => ({ id: `wall-${terminalId}`, sessionId, terminalId });

describe("Broadcast's tiles on the Wall", () => {
  it("puts each ticked server's terminal on the Wall, in the order they're ticked", () => {
    const r = syncBroadcastTiles([], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t1" }], new Set());
    expect(r.tiles).toEqual([tile("a", "a-t0"), tile("b", "b-t1")]);
    expect([...r.ours]).toEqual(["wall-a-t0", "wall-b-t1"]);
  });

  it("adds a server ticked later, and takes off one unticked, leaving the rest where they are", () => {
    let r = syncBroadcastTiles([], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }], new Set());
    r = syncBroadcastTiles(r.tiles, [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }, { sessionId: "c", terminalId: "c-t0" }], r.ours);
    expect(r.tiles.map((t) => t.sessionId)).toEqual(["a", "b", "c"]);
    r = syncBroadcastTiles(r.tiles, [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "c", terminalId: "c-t0" }], r.ours);
    expect(r.tiles.map((t) => t.sessionId)).toEqual(["a", "c"]);
  });

  it("leaves terminals pinned by hand alone, when Broadcast ends too", () => {
    const pinned = tile("x", "x-t0");
    let r = syncBroadcastTiles([pinned], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }], new Set());
    expect(r.tiles).toEqual([pinned, tile("a", "a-t0"), tile("b", "b-t0")]);
    r = syncBroadcastTiles(r.tiles, [], r.ours);
    expect(r.tiles).toEqual([pinned]);
    expect(r.ours.size).toBe(0);
  });

  it("doesn't count a terminal pinned by hand before as its own, so it stays after", () => {
    const pinned = tile("a", "a-t0");
    let r = syncBroadcastTiles([pinned], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }], new Set());
    expect(r.tiles).toEqual([pinned, tile("b", "b-t0")]);
    r = syncBroadcastTiles(r.tiles, [], r.ours);
    expect(r.tiles).toEqual([pinned]);
  });

  it("follows the terminal in front when a server's tab shows another one", () => {
    let r = syncBroadcastTiles([], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }], new Set());
    r = syncBroadcastTiles(r.tiles, [{ sessionId: "a", terminalId: "a-t1" }, { sessionId: "b", terminalId: "b-t0" }], r.ours);
    expect(r.tiles.map((t) => t.terminalId).sort()).toEqual(["a-t1", "b-t0"]);
  });

  it("waits for a server whose terminal hasn't opened yet, and changes nothing when nothing changed", () => {
    let r = syncBroadcastTiles([], [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b" }], new Set());
    expect(r.tiles.map((t) => t.sessionId)).toEqual(["a"]);
    const same = syncBroadcastTiles(r.tiles, [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b" }], r.ours);
    expect(same.tiles).toBe(r.tiles);
    r = syncBroadcastTiles(r.tiles, [{ sessionId: "a", terminalId: "a-t0" }, { sessionId: "b", terminalId: "b-t0" }], r.ours);
    expect(r.tiles.map((t) => t.sessionId)).toEqual(["a", "b"]);
  });
});

describe("the Broadcast menu's switch", () => {
  const bc = (enabled: boolean, ticked: string[]) => ({
    enabled, targetSessionIds: new Set(ticked), sessionTerminalMap: {},
    enable: vi.fn(), disable: vi.fn(), selectAll: vi.fn(), selectNone: vi.fn(),
  });
  it("ticks every open session when off, and unticks them all when on", () => {
    const off = bc(false, []);
    broadcastSwitch(off, ["a", "b", "c"]);
    expect(off.selectAll).toHaveBeenCalledWith(["a", "b", "c"]);
    const on = bc(true, ["a", "b"]);
    broadcastSwitch(on, ["a", "b", "c"]);
    expect(on.selectNone).toHaveBeenCalled();
    // Ticked but not yet on (just after a restart): the switch clears them.
    const stale = bc(false, ["a", "b"]);
    broadcastSwitch(stale, ["a", "b"]);
    expect(stale.selectNone).toHaveBeenCalled();
  });
});

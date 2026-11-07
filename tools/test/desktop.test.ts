// The rules behind the fork's parts of the main window (src/fork/desktop/rules.ts,
// moved out of DesktopApp.tsx, the original project's file, for #163).
import { describe, expect, it } from "vitest";
import {
  type DesktopSession, aiPaneViewOf, closeCounts, osChecksDue, sessionFor, sessionPaneStyle, stepTab, tabMenuStyle, tabsToClose, wouldEnd,
} from "../../src/fork/desktop/rules";

describe("wouldEnd", () => {
  it("asks nothing for no sessions and Home's one terminal, which the app always has", () => {
    expect(wouldEnd(0, 0)).toBeNull();
    expect(wouldEnd(0, 1)).toBeNull();
  });

  it("names what closing ends, and whether it's one or many", () => {
    expect(wouldEnd(1, 0)).toEqual({ what: "1 SSH session", many: false });
    expect(wouldEnd(0, 2)).toEqual({ what: "2 terminals on this computer", many: true });
    expect(wouldEnd(3, 1)).toEqual({ what: "3 SSH sessions and 1 terminal on this computer", many: true });
  });
});

describe("closing tabs from a tab's menu", () => {
  const tabs = ["a", "b", "c", "d"];
  const statuses = { a: "connected", b: "failed", c: "disconnected" }; // d: still connecting

  it("closes the others, those to the right, the disconnected ones, or all, in title-bar order", () => {
    expect(tabsToClose("others", tabs, "b", statuses)).toEqual(["a", "c", "d"]);
    expect(tabsToClose("right", tabs, "b", statuses)).toEqual(["c", "d"]);
    expect(tabsToClose("dead", tabs, "b", statuses)).toEqual(["b", "c"]);
    expect(tabsToClose("all", tabs, "b", statuses)).toEqual(tabs);
  });

  it("counts what each item would close, so an empty one is greyed out", () => {
    expect(closeCounts(tabs, "d", statuses)).toEqual({ others: 3, right: 0, dead: 2, all: 4 });
    // A tab that's gone meanwhile: nothing is "other" than it, nothing to its right.
    expect(closeCounts(tabs, "x", statuses)).toEqual({ others: 4, right: 0, dead: 2, all: 4 });
  });
});

describe("stepTab", () => {
  const order = ["home", "s1", "s2"];

  it("steps through the tabs and round", () => {
    expect(stepTab(order, "home", 1)).toBe("s1");
    expect(stepTab(order, "s2", 1)).toBe("home");
    expect(stepTab(order, "home", -1)).toBe("s2");
  });

  it("goes to the first or last tab from a view that isn't one", () => {
    expect(stepTab(order, "settings", 1)).toBe("home");
    expect(stepTab(order, "settings", -1)).toBe("s2");
    expect(stepTab([], "settings", 1)).toBeNull();
  });
});

describe("sessionPaneStyle", () => {
  const place = { position: "absolute" as const, top: 0, bottom: 0, left: "50%", width: "50%" };

  it("keeps a tab on screen where it is", () => {
    expect(sessionPaneStyle(place, true, "ssh")).toEqual(place);
  });

  it("takes a terminal tab that isn't on screen out of the layout, so xterm stops drawing it", () => {
    expect(sessionPaneStyle(place, false, undefined)).toEqual({ ...place, display: "none" });
    expect(sessionPaneStyle(undefined, false, "telnet")).toEqual({ position: "absolute", inset: 0, display: "none" });
  });

  it("keeps a remote desktop's size, only hidden, for the desktop it asks the server for", () => {
    expect(sessionPaneStyle(place, false, "rdp")).toEqual(place);
    expect(sessionPaneStyle(undefined, false, "vnc")).toEqual({ position: "absolute", inset: 0, opacity: 0, pointerEvents: "none" });
  });
});

describe("tabMenuStyle", () => {
  it("opens at the click when there's room, and moves up and scrolls when there isn't", () => {
    expect(tabMenuStyle(100, 40, 1200, 800)).toEqual({ left: 100, top: 40, maxHeight: 752 });
    expect(tabMenuStyle(1100, 700, 1200, 800)).toEqual({ left: 940, top: 420, maxHeight: 372 });
    // A window shorter than the menu: it starts at the top, and scrolls.
    expect(tabMenuStyle(10, 200, 1200, 300)).toEqual({ left: 10, top: 8, maxHeight: 284 });
  });
});

describe("opening a saved server", () => {
  const server = {
    id: 7, name: "web", host: "10.0.0.7", port: 2222, protocol: "kerberos", uuid: "u7", mirrors: "[]",
    run_on_connect: "uptime", run_on_connect_every: 1, held: ["run_on_connect"],
  };

  it("makes its tab with the host and port it opens with, and an unknown protocol as SSH", () => {
    expect(sessionFor(server)).toEqual({
      id: "session-7", serverId: 7, serverName: "web", protocol: "ssh", host: "10.0.0.7", port: 2222, mirrors: "[]",
      serverUuid: "u7", runOnConnect: "uptime", runOnConnectEvery: true, runOnConnectHeld: true,
    });
    expect(sessionFor({ id: 1, name: "desk", protocol: "rdp" })).toMatchObject({ protocol: "rdp", serverUuid: null, runOnConnectHeld: false });
  });
});

describe("osChecksDue", () => {
  const tab = (id: string, serverId: number, extra: Partial<DesktopSession> = {}): DesktopSession =>
    ({ id, serverId, serverName: "s", host: "h", port: 22, ...extra });
  const servers = [{ id: 1, port: 22 }, { id: 2, port: 22, os: "debian" }, { id: 3, port: 2200 }];
  const connected = { a: "connected", b: "connected", c: "connected", d: "connecting", e: "connected", f: "connected" };

  it("asks a connected SSH server whose OS isn't known, once, by the host and port the tab opened with", () => {
    const sessions = [
      tab("a", 1),
      tab("b", 1), // the same server, host and port again
      tab("c", 2), // OS known
      tab("d", 3), // not connected yet
      tab("e", 3, { protocol: "rdp" }),
      tab("f", 3, { port: undefined }), // the server's port, then
    ];
    expect(osChecksDue(sessions, connected, servers, new Set())).toEqual([
      { key: "1|h|22", sessionId: "a", serverId: 1, host: "h", port: 22 },
      { key: "3|h|2200", sessionId: "f", serverId: 3, host: "h", port: 2200 },
    ]);
  });

  it("doesn't ask again this run, nor ask for a quick connect or a server that's gone", () => {
    expect(osChecksDue([tab("a", 1)], connected, servers, new Set(["1|h|22"]))).toEqual([]);
    expect(osChecksDue([tab("a", 0), tab("b", 9)], connected, servers, new Set())).toEqual([]);
  });
});

describe("aiPaneViewOf", () => {
  const sessions: DesktopSession[] = [{ id: "session-1", serverId: 1, serverName: "web" }];

  it("describes the server tab on screen, by its tab name and the terminal in front", () => {
    expect(aiPaneViewOf("session-1", sessions, { "session-1": "connected" }, {}, () => "Web (prod)")).toEqual({
      view: "session-1",
      session: { id: "session-1", name: "Web (prod)", ssh: true, connected: true, terminalId: "session-1-term-0" },
    });
    expect(aiPaneViewOf("session-1", sessions, {}, { "session-1": "t2" }, (s) => s.serverName).session)
      .toMatchObject({ connected: false, terminalId: "t2" });
  });

  it("has no session for a view that isn't a server tab", () => {
    expect(aiPaneViewOf("home", sessions, {}, {}, (s) => s.serverName)).toEqual({ view: "home", session: null });
  });
});

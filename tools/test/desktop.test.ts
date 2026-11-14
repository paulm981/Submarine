// The rules behind the fork's parts of the main window (src/fork/desktop/rules.ts,
// moved out of DesktopApp.tsx, the original project's file, for #163).
import { describe, expect, it } from "vitest";
import {
  type DesktopSession, aiPaneViewOf, closeCounts, inRemoteDesktop, osChecksDue, sessionFor, sessionPaneStyle, stepTab, tabDivider, tabMenuStyle,
  tabsToClose, wouldEnd,
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

  // From the session tree or a folder, an FTP server opens in its file
  // view (DesktopApp's FtpSessionView), as from the grid: not as SSH on 21.
  it("keeps an FTP server's kind", () => {
    for (const protocol of ["ftp", "ftps", "ftps-implicit", "telnet", "vnc"]) {
      expect(sessionFor({ id: 2, name: "files", host: "f.example", port: 21, protocol }).protocol).toBe(protocol);
    }
    expect(sessionFor({ id: 3, name: "web", protocol: null }).protocol).toBe("ssh");
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
  // An FTP tab has no SSH session: it can't be shared with the AI (#246).
  it("says an FTP tab isn't SSH, opened from the tree or not", () => {
    for (const protocol of ["ftp", "ftps", "ftps-implicit"]) {
      const tab = sessionFor({ id: 4, name: "files", host: "f.example", port: 21, protocol });
      expect(aiPaneViewOf(tab.id, [tab], { [tab.id]: "connected" }, {}, (s) => s.serverName).session).toMatchObject({ ssh: false });
      expect(osChecksDue([tab], { [tab.id]: "connected" }, [{ id: 4, port: 21 }], new Set())).toEqual([]);
    }
  });
});

describe("the title bar's tabs (the original project's classic tabs, #339)", () => {
  const order = ["home", "wall", "s1", "s2"];

  it("draws a divider between two tabs where neither is the open one", () => {
    expect(tabDivider(order, "s2", "home")).toBe(true);
    expect(tabDivider(order, "s2", "wall")).toBe(true);
    // Not on the open tab, nor on the one just before it, nor on the last.
    expect(tabDivider(order, "s2", "s2")).toBe(false);
    expect(tabDivider(order, "s2", "s1")).toBe(false);
    expect(tabDivider(order, "home", "s2")).toBe(false);
    // A tab that isn't in the strip (Broadcast off) has none.
    expect(tabDivider(["home", "s1"], "s1", "wall")).toBe(false);
  });

  it("leaves Ctrl+Tab to a remote desktop, and only there", () => {
    const inside = { closest: (sel: string) => (sel === "[data-remote-keys]" ? {} : null) } as unknown as EventTarget;
    const outside = { closest: () => null } as unknown as EventTarget;
    expect(inRemoteDesktop(inside)).toBe(true);
    expect(inRemoteDesktop(outside)).toBe(false);
    expect(inRemoteDesktop(null)).toBe(false);
    // The window itself has no closest().
    expect(inRemoteDesktop({} as EventTarget)).toBe(false);
  });

  // The original project's #339 added its own tab drag and Ctrl+Tab. Both
  // merged into DesktopApp.tsx without a conflict and would have beaten the
  // fork's: a later onPointerDown wins over the fork's drag spread, and a
  // capture-phase Ctrl+Tab listener runs before Fork.useTabKeys, skipping
  // Home and Broadcast and pulling servers out of Broadcast. This fails if a
  // sync brings them back.
  const SOURCES = import.meta.glob<string>("/src/DesktopApp.tsx", { query: "?raw", import: "default", eager: true });
  const code = (SOURCES["/src/DesktopApp.tsx"] ?? "")
    .replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "").replace(/\s\/\/ .*$/gm, "");

  it("drags a server tab with the fork's drag only", () => {
    const lines = code.split("\n");
    const at = lines.findIndex((l) => l.includes("{...sessionDrag.tabProps(s.id)}"));
    expect(at, "DesktopApp no longer spreads the fork's tab drag").toBeGreaterThanOrEqual(0);
    const end = lines.findIndex((l, i) => i > at && l.trim() === ">");
    const tab = lines.slice(at + 1, end).join("\n");
    expect(tab).not.toMatch(/onPointerDown=/);
    expect(code).not.toMatch(/startTabDrag|setTabOrder|tabDragged/);
  });

  it("has one Ctrl+Tab, the fork's", () => {
    expect(code).toMatch(/Fork\.useTabKeys\(/);
    expect(code).not.toMatch(/e\.key\s*!==?\s*["']Tab["']/);
    expect(code).not.toMatch(/addEventListener\(["']keydown["'][^)]*,\s*true\)/);
  });

  it("closes a tab, from its ×, a middle click, its menu or the phone's list, through the fork's close", () => {
    expect(code).toMatch(/const closeTab = async \(sid: string, ask: boolean\) => \{ await closeSessions\(\[sid\], ask\); \};/);
    expect(code).toMatch(/onAuxClick=\{\(e\) => \{\s*if \(e\.button !== 1\) return;\s*e\.preventDefault\(\);\s*void closeTab\(s\.id, true\);/);
  });
});

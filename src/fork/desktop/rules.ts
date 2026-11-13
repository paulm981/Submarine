import type { CSSProperties } from "react";
import type { QuickAuth } from "../../components/QuickConnectModal";
import type { AiPaneView } from "../../components/AiSidePane";
import { IS_ANDROID } from "../../util/platform";
import { type ServerProtocol } from "../../util/serverProtocol";

// ─────────────────────────────────────────────────────────────────────────────
// The rules behind the fork's parts of the main window. DesktopApp.tsx is the
// original project's file, so the fork keeps its code out of it and calls in
// with one-line hooks (CLAUDE.md, "Fork features that don't cost sync time";
// #163). This file is the part with no React or Tauri at run time, so the page's
// tests can drive it (tools/test/desktop.test.ts).
// ─────────────────────────────────────────────────────────────────────────────

/** The Home view (HomePane): terminals on this computer, where the app opens
 *  and where closing the last session tab goes back to. Android has no
 *  shells to offer, so it keeps the server grid there. */
export const HAS_HOME = !IS_ANDROID;
export const START_VIEW = HAS_HOME ? "home" : "nodes";

/** A server tab, as DesktopApp keeps it (its `Session`). `host` and `port`
 *  are what the tab was opened with, which the OS badge check goes by (an
 *  edit made while the tab is open doesn't move them). */
export type DesktopSession = {
  id: string; serverId: number; serverName: string; serverUuid?: string | null;
  protocol?: ServerProtocol; host?: string; port?: number; mirrors?: string;
  runOnConnect?: string; runOnConnectEvery?: boolean; runOnConnectHeld?: boolean; quickAuth?: QuickAuth | null;
};

/** The tab for a saved server, as opening it makes it. */
export const sessionFor = (server: any): DesktopSession => ({
  id: `session-${server.id}`,
  serverId: server.id,
  serverName: server.name,
  protocol: ["rdp", "telnet", "vnc"].includes(server.protocol) ? server.protocol : "ssh",
  host: server.host,
  port: server.port,
  // Pass the raw mirrors JSON through to SessionView so the
  // MirrorsPanel can pre-populate "Saved on this node" without
  // another round-trip to the backend.
  mirrors: server.mirrors,
  serverUuid: server.uuid ?? null,
  // Commands typed into the first terminal on connect (see SessionView).
  runOnConnect: server.run_on_connect || "",
  // And into each other terminal of the tab (#256).
  runOnConnectEvery: !!server.run_on_connect_every,
  // Set or changed by someone this profile is shared with, not allowed
  // here yet: SessionView asks before typing them.
  runOnConnectHeld: (server.held || []).includes("run_on_connect"),
});

/** What closing the window or locking the profile would end, for the
 *  question asked first, or null when there's nothing to ask about. One Home
 *  shell on its own doesn't count: the app always has one, and every lock
 *  would ask. */
export const wouldEnd = (ssh: number, local: number): { what: string; many: boolean } | null => {
  if (ssh === 0 && local < 2) return null;
  const parts = [];
  if (ssh > 0) parts.push(`${ssh} SSH session${ssh === 1 ? "" : "s"}`);
  if (local > 0) parts.push(`${local} terminal${local === 1 ? "" : "s"} on this computer`);
  return { what: parts.join(" and "), many: ssh + local > 1 };
};

/** A tab's right-click menu closes the tabs other than it, after it, the
 *  disconnected ones, or every one. */
export type CloseWhich = "others" | "right" | "dead" | "all";

const isDead = (status: string | undefined) => ["failed", "disconnected"].includes(status ?? "connecting");

/** The tabs `which` closes, in title-bar order (`tabs`). */
export const tabsToClose = (which: CloseWhich, tabs: string[], targetId: string, statuses: Record<string, string>): string[] => {
  const at = tabs.indexOf(targetId);
  return which === "others" ? tabs.filter(id => id !== targetId) :
    which === "right" ? tabs.slice(at + 1) :
    which === "dead" ? tabs.filter(id => isDead(statuses[id])) :
    [...tabs];
};

/** How many tabs each close item in the menu would close. */
export const closeCounts = (tabs: string[], targetId: string, statuses: Record<string, string>): Record<CloseWhich, number> => {
  const at = tabs.indexOf(targetId);
  return {
    others: tabs.length - (at >= 0 ? 1 : 0),
    right: at >= 0 ? tabs.length - at - 1 : 0,
    dead: tabs.filter(id => isDead(statuses[id])).length,
    all: tabs.length,
  };
};

/** Ctrl+Tab (1) and Ctrl+Shift+Tab (-1) through the title bar's tabs, left
 *  to right and round: the tab they go to, or null when there are none.
 *  From a view that isn't a tab (Settings, the vault…) they go to the first
 *  or the last. */
export const stepTab = (order: string[], current: string, dir: 1 | -1): string | null => {
  if (order.length === 0) return null;
  const at = order.indexOf(current);
  return order[at < 0 ? (dir > 0 ? 0 : order.length - 1) : (at + dir + order.length) % order.length];
};

/** Where a server tab's view goes in the sessions canvas. `style` is the
 *  place it has when it's on screen (in a split, a column of it), undefined
 *  when it isn't.
 *
 *  A session with terminals that isn't on screen is left out of the layout
 *  (display: none), not only made invisible: xterm stops drawing only then,
 *  and 35 hidden tabs drawing their output slowed the one in front to a few
 *  frames a second (#127). Its shell and buffer keep running. A remote
 *  desktop keeps its size instead, for the desktop it asks the server for. */
export const sessionPaneStyle = (
  style: CSSProperties | undefined,
  onScreen: boolean,
  protocol: string | undefined,
): CSSProperties => {
  const remote = protocol === "rdp" || protocol === "vnc";
  if (style) return { ...style, ...(onScreen || remote ? {} : { display: "none" }) };
  return remote
    ? { position: "absolute", inset: 0, opacity: 0, pointerEvents: "none" }
    : { position: "absolute", inset: 0, display: "none" };
};

/** Where a tab's right-click menu goes. With the split list and the close
 *  items it can be taller than the space under the click: it moves up as
 *  far as it can, and scrolls inside what's left. */
export const tabMenuStyle = (x: number, y: number, width: number, height: number): CSSProperties => {
  const top = Math.max(8, Math.min(y, height - 380));
  return { left: Math.min(x, width - 260), top, maxHeight: height - top - 8 };
};

/** What the AI side pane needs of the view on screen. */
export const aiPaneViewOf = (
  view: string,
  sessions: DesktopSession[],
  statuses: Record<string, string>,
  activeTerminals: Record<string, string>,
  label: (s: DesktopSession) => string,
): AiPaneView => {
  const s = sessions.find(x => x.id === view);
  return {
    view,
    session: s ? {
      id: s.id,
      name: label(s),
      ssh: (s.protocol ?? "ssh") === "ssh",
      connected: statuses[s.id] === "connected",
      // The terminal on screen in the tab.
      terminalId: activeTerminals[s.id] ?? `${s.id}-term-0`,
    } : null,
  };
};

/** A server to ask for its OS (server_os.rs), and the key it's asked by. */
export interface OsCheck { key: string; sessionId: string; serverId: number; host: string; port: number }

/** The servers to ask for their OS now: an SSH tab that's connected, to a
 *  saved server whose OS isn't known yet, by server, host and port, unless
 *  `checked` has it (asked this run already). The host and port are the
 *  ones the tab connected to: the server may have been edited since. */
export const osChecksDue = (
  sessions: DesktopSession[],
  statuses: Record<string, string>,
  servers: any[],
  checked: ReadonlySet<string>,
): OsCheck[] => {
  const due: OsCheck[] = [];
  for (const s of sessions) {
    // Only an SSH server has a shell to ask (not an RDP one).
    if (s.serverId <= 0 || !s.host || (s.protocol ?? "ssh") !== "ssh" || statuses[s.id] !== "connected") continue;
    const server = servers.find((x: any) => x.id === s.serverId);
    if (!server || server.os) continue;
    const port = s.port ?? server.port;
    const key = `${server.id}|${s.host}|${port}`;
    if (checked.has(key) || due.some(d => d.key === key)) continue;
    due.push({ key, sessionId: s.id, serverId: s.serverId, host: s.host, port });
  }
  return due;
};

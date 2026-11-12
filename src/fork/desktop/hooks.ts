import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { TreeItems } from "../../components/SessionTree";
import { useBroadcast } from "../../ui/broadcast";
import { broadcastLive, shownTabs } from "./broadcastWall";
import { useConfirm } from "../../ui/confirm";
import { normalizeRules, onSlowRule, setCustomRules, setServerHighlights } from "../../ui/highlight";
import { reorderById, useTabDrag } from "../../ui/tabDrag";
import { appHotkey } from "../../ui/terminalDock";
import { setServerSchemes } from "../../ui/terminalThemes";
import { OPEN_SERVER } from "../../util/ai";
import { HOME_HERE } from "../terminal/here";
import { logLine } from "../../util/diagnostics";
import { visibleName } from "../../util/fileName";
import { folderTree } from "../../util/folderTree";
import { readRdpOptions, writeRdpOptions } from "../../util/rdpOptions";
import { readSessionKeep, writeSessionKeep } from "../../util/sessionKeep";
import {
  type CloseWhich, type DesktopSession, HAS_HOME, START_VIEW, osChecksDue, sessionFor, stepTab, tabsToClose,
} from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state and effects in the main window, as hooks DesktopApp calls
// in one line each (#163): DesktopApp.tsx is the original project's file, so
// every line the fork adds there can conflict at the next sync. Each hook
// does what DesktopApp did in its place, in the same order.
// ─────────────────────────────────────────────────────────────────────────────

type AddLog = (msg: string, type?: string) => void;
type SetList<T> = (f: (prev: T[]) => T[]) => void;

/** Servers' own terminal colors and highlighting: open terminals follow when
 *  one changes (an edit here, or one that came in with a sync). */
export const useServerLooks = (servers: any[]) => {
  useEffect(() => {
    const schemes = setServerSchemes(servers);
    const highlights = setServerHighlights(servers);
    if (schemes || highlights) window.dispatchEvent(new CustomEvent('submarine-settings-changed'));
  }, [servers]);
};

/** The user's own highlight rules (synced with the profile): what reloads
 *  them, so open terminals pick up a change. A rule too slow for some output
 *  is turned off, and the log says so. */
export const useHighlightRules = (addLog: AddLog) => {
  const refreshHighlightRules = useCallback(async () => {
    try {
      if (setCustomRules(normalizeRules(await invoke("highlight_rules_list")))) {
        window.dispatchEvent(new CustomEvent('submarine-settings-changed'));
      }
    } catch (e) { addLog(`SYNC_HIGHLIGHT_RULES: ${e}`, "error"); }
  }, [addLog]);
  useEffect(() => {
    onSlowRule((label) => {
      const msg = `The highlight rule "${label}" was too slow on this output, so it's off until Submarine restarts.`;
      addLog(msg, "warn");
      logLine("warn", `[highlight] ${msg}`);
    });
  }, [addLog]);
  return refreshHighlightRules;
};

/** The review dialog's answers in the server list, beyond the original
 *  project's allowHeld: `allowHeldSeen` allows on-connect commands only as
 *  they were shown (`runOnConnect`, and `runOnConnectEvery` whether they go
 *  into each new terminal): when either changed meanwhile the OK is refused
 *  and the review shows them as they are now. `keepServerSsh` declines a
 *  held switch to telnet or VNC: the server stays SSH (or RDP), and the
 *  other computers get it back as that with the next sync. `keepAiPolicy`
 *  keeps a teammate's move that let the AI do more on a server as it was:
 *  the server stays at "Ask every time" here until it's changed (#115). */
export const useHeldActions = (addLog: AddLog, refreshServers: () => Promise<void>) => {
  const allowHeldSeen = useCallback(async (serverId: number, items: string[], runOnConnect?: string, runOnConnectEvery?: boolean) => {
    try {
      await invoke("allow_held", { serverId, items, runOnConnect: runOnConnect ?? null, runOnConnectEvery: runOnConnectEvery ?? null });
      addLog(items.length ? `Allowed ${items.length} held change${items.length === 1 ? "" : "s"}.` : "Allowed the held changes.", "success");
    } catch (e) {
      if (String(e).includes("HELD_CHANGED")) addLog("Those on-connect commands were changed again. Look at them again before you allow them.", "warn");
      else addLog(`ALLOW_FAILED: ${e}`, "error");
    }
    await refreshServers();
  }, [addLog, refreshServers]);
  const keepServerSsh = useCallback(async (serverId: number) => {
    try {
      if (await invoke<boolean>("keep_server_ssh", { serverId })) addLog("Kept the server as it was.", "success");
    } catch (e) {
      addLog(`KEEP_SSH_FAILED: ${e}`, "error");
    }
    await refreshServers();
  }, [addLog, refreshServers]);
  const keepAiPolicy = useCallback(async (serverId: number) => {
    try {
      await invoke("ai_keep_policy", { serverId });
      addLog("The AI asks before every command on that server, as before.", "success");
    } catch (e) {
      addLog(`AI_KEEP_FAILED: ${e}`, "error");
    }
    await refreshServers();
  }, [addLog, refreshServers]);
  return { allowHeldSeen, keepServerSsh, keepAiPolicy };
};

/** The title bar's tab strip: it scrolls the tab on screen into view (with
 *  dozens open, a tab picked from All tabs lands past the edge), and
 *  dragging a server tab to a new place reorders `sessions` itself, so
 *  Ctrl+Tab and All tabs follow it. */
export const useTabStrip = <S extends { id: string }>(activeView: string, sessions: S[], setSessions: SetList<S>) => {
  const tabStripRef = useRef<HTMLDivElement>(null);
  const sessionDrag = useTabDrag("sessions", (dragId, overId, after) =>
    setSessions(prev => reorderById(prev, dragId, overId, after)));
  useEffect(() => {
    const strip = tabStripRef.current;
    if (!strip) return;
    const tab = [...strip.querySelectorAll<HTMLElement>("[data-tab]")].find(el => el.dataset.tab === activeView);
    tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeView, sessions]);
  return { tabStripRef, sessionDrag };
};

/** Live Edit copies with changes that were never uploaded, kept when their
 *  session ended or the profile was locked (keep_unsent_edits in lib.rs):
 *  the log says where, and a prompt offers the folder. */
export const useLiveEditKept = (addLog: AddLog) => {
  const confirm = useConfirm();
  useEffect(() => {
    const unlisten = listen<{ folder: string; files: string[] }>("live-edit-kept", async (e) => {
      const { folder, files } = e.payload;
      const what = files.length === 1 ? `${visibleName(files[0])} has changes` : `${files.length} files have changes`;
      addLog(`Live Edit: ${what} that were never uploaded to the server. Kept in ${folder}.`, "info");
      const ok = await confirm({
        title: "Unsent edits kept",
        message: `${what} that were never uploaded to the server. ${files.length === 1 ? "It's" : "They're"} kept in:\n\n${folder}`,
        okLabel: "Show folder",
        cancelLabel: "OK",
      });
      if (ok) invoke("local_open_in_explorer", { localPath: folder }).catch(() => {});
    });
    return () => { unlisten.then((f) => f()).catch(() => {}); };
  }, [addLog, confirm]);
};

/** Each server's OS, for the badges in the session tree and the server grid
 *  (src-tauri/src/server_os.rs): read once, when a server whose OS isn't
 *  known yet connects, unless Settings turns it off. Moving a server to
 *  another host or port forgets its OS, so it is read again; a server that
 *  can't answer isn't asked again until the app restarts, or the profile is
 *  locked (`profile` null). */
export const useOsBadges = (o: {
  profile: string | null; sessions: DesktopSession[]; statuses: Record<string, string>; servers: any[];
  detectOs: boolean; refreshServers: () => Promise<void>;
}) => {
  // The servers asked for their OS this run, by server, host and port.
  const checked = useRef<Set<string>>(new Set());
  useEffect(() => { if (!o.profile) checked.current.clear(); }, [o.profile]);
  useEffect(() => {
    if (!o.detectOs) return;
    for (const c of osChecksDue(o.sessions, o.statuses, o.servers, checked.current)) {
      checked.current.add(c.key);
      invoke<string | null>("detect_server_os", { sessionId: c.sessionId, serverId: c.serverId, host: c.host, port: c.port })
        .then((os) => { if (os) o.refreshServers(); })
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [o.statuses, o.sessions, o.servers, o.detectOs]);
};

/** Closing tabs: one connected session asks first, unless that's turned off
 *  in Settings › Servers, or with "Don't ask again" right in the prompt
 *  (`askCloseSession`). `closeSessions` closes several (All tabs ▾), always
 *  asking first when any is connected; false when the user said no.
 *  Unmounting a SessionView disconnects it. `closeTabs` is a tab's
 *  right-click menu: the tabs other than it, after it, the disconnected
 *  ones, or every one. */
export const useCloseSessions = <S extends { id: string }, T extends { confirmCloseSession: boolean }>(o: {
  sessions: S[]; statuses: Record<string, string>;
  confirmClose: boolean; setAppSettings: (f: (s: T) => T) => void;
  clearTabName: (id: string) => void; setSessions: SetList<S>;
  setSessionStatuses: (f: (prev: Record<string, string>) => Record<string, string>) => void;
  setMergedSessionIds: SetList<string>; broadcast: { removeSession: (id: string) => void };
  unpinSessionFromWall: (id: string) => void; setActiveView: (f: (prev: string) => string) => void;
}) => {
  const confirm = useConfirm();
  const askCloseSession = () => (!o.confirmClose ? Promise.resolve(true) : confirm({
    title: "Close this session?",
    message: "The connection will be dropped.",
    okLabel: "Close",
    destructive: true,
    dontAskAgain: () => o.setAppSettings(s => ({ ...s, confirmCloseSession: false })),
  }));
  const closeSessions = async (ids: string[]): Promise<boolean> => {
    if (ids.length === 0) return true;
    const connected = ids.filter(id => o.statuses[id] === "connected").length;
    if (connected > 0) {
      const ok = ids.length === 1 ? await askCloseSession() : await confirm({
        title: `Close ${ids.length} sessions?`,
        message: connected === 1 ? "1 of them is connected; its SSH connection will be dropped." : `${connected} of them are connected; ${connected} SSH connections will be dropped.`,
        okLabel: "Close",
        destructive: true,
      });
      if (!ok) return false;
    }
    const closing = new Set(ids);
    for (const id of closing) o.clearTabName(id);
    o.setSessions(prev => prev.filter(s => !closing.has(s.id)));
    o.setSessionStatuses(prev => {
      const next = { ...prev };
      for (const id of closing) delete next[id];
      return next;
    });
    o.setMergedSessionIds(prev => {
      const next = prev.filter(id => !closing.has(id));
      return next.length < 2 ? [] : next;
    });
    for (const id of closing) {
      o.broadcast.removeSession(id);
      o.unpinSessionFromWall(id);
    }
    o.setActiveView(prev => (closing.has(prev) ? START_VIEW : prev));
    return true;
  };
  const closeTabs = async (which: CloseWhich, targetId: string) => {
    await closeSessions(tabsToClose(which, o.sessions.map(s => s.id), targetId, o.statuses));
  };
  return { askCloseSession, closeSessions, closeTabs };
};

/** The keys for moving between tabs: Ctrl+Shift+K opens All tabs ▾ to jump
 *  by name (its search is focused); Ctrl+Tab / Ctrl+Shift+Tab step through
 *  the title bar left to right: Home, Broadcast's tab while it's on, then the server tabs (stepTab).
 *  preventDefault keeps the webview from doing anything of its own with
 *  Ctrl+Tab. Returns whether All tabs ▾ is open. */
export const useTabKeys = (o: {
  isUnlocked: boolean; sessions: { id: string }[]; activeView: string;
  mergedSessionIds: string[]; setMergedSessionIds: (ids: string[]) => void; setActiveView: (v: string) => void;
}) => {
  const [allTabsOpen, setAllTabsOpen] = useState(false);
  // The tabs as the strip shows them: Broadcast's servers folded into its tab (#224).
  const bc = useBroadcast();
  const order = [
    ...(HAS_HOME && o.isUnlocked ? ["home"] : []),
    ...(broadcastLive(bc) ? ["wall"] : []),
    ...shownTabs(o.sessions, bc).map(s => s.id),
  ];
  // Set anew each render, so they go by the tabs and the split as they are.
  const stepTabRef = useRef((_dir: 1 | -1) => {});
  stepTabRef.current = (dir) => {
    const next = stepTab(order, o.activeView, dir);
    if (next === null) return;
    // Leaves a split the way a plain click on a tab does: only for a tab
    // that isn't part of it.
    if (!o.mergedSessionIds.includes(next)) o.setMergedSessionIds([]);
    o.setActiveView(next);
  };
  const canJumpRef = useRef(false);
  canJumpRef.current = o.isUnlocked && o.sessions.length > 0;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const k = appHotkey(e);
      if (k !== "jump" && k !== "next" && k !== "prev") return;
      e.preventDefault();
      if (e.repeat && k === "jump") return;
      // Not under a dialog: what it asks about is the view as it is.
      if (document.querySelector('[aria-modal="true"]')) return;
      if (k === "jump") { if (canJumpRef.current) setAllTabsOpen(true); }
      else stepTabRef.current(k === "next" ? 1 : -1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return [allTabsOpen, setAllTabsOpen] as const;
};

/** "Pick a jump host", from a tab whose server's jump host was deleted
 *  (SessionView): its form, at Jump via. Returns whether the form is to
 *  open there (`edit` sets it, for every server it opens). */
export const useJumpHostPick = (servers: any[], edit: (server: any, opts: { focusJump: boolean }) => unknown) => {
  const focus = useState(false);
  useEffect(() => {
    const onPick = (e: Event) => {
      const id = (e as CustomEvent<{ serverId: number }>).detail?.serverId;
      const server = (servers || []).find((x: any) => x.id === id);
      if (server) void edit(server, { focusJump: true });
    };
    window.addEventListener("submarine-pick-jump-host", onPick);
    return () => window.removeEventListener("submarine-pick-jump-host", onPick);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [servers]);
  return focus;
};

const readPref = (key: string, fallback: string) => {
  try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; }
};
const writePref = (key: string, value: string) => {
  try { localStorage.setItem(key, value); } catch { /* per-device nicety only */ }
};

/** The session tree (desktop): shown or hidden, and on which side (per
 *  device, like the other UI preferences), and everything it does. Also
 *  what the server grid shares with it: making a folder and cloning a
 *  server. */
export const useSessionTree = (o: {
  profile: string | null; servers: any[]; folders: any[]; sessions: DesktopSession[];
  statuses: Record<string, string>; waiting: Set<string>; activeView: string;
  setSessions: SetList<DesktopSession>; setActiveView: (v: string) => void; setMergedSessionIds: (ids: string[]) => void;
  allowHeld: (serverId: number, items: string[]) => Promise<void>;
  editServer: (server: any) => void; renameFolder: (id: number, name: string) => Promise<void>;
  newServer: { setNewNode: (node: any) => void; defaultNode: any; setFormError: (e: string) => void; setIsPanelOpen: (open: boolean) => void };
  refreshFolders: () => Promise<void>; refreshServers: () => Promise<void>; bumpSync: () => void; addLog: AddLog;
  forgetServer: (s: { id: number; uuid?: string | null }) => void;
}) => {
  const confirm = useConfirm();
  const [open, setOpen] = useState(() => readPref("submarine-session-tree-open", "1") === "1");
  const [side, setSide] = useState<"left" | "right">(() => (readPref("submarine-session-tree-side", "right") === "left" ? "left" : "right"));
  useEffect(() => writePref("submarine-session-tree-open", open ? "1" : "0"), [open]);
  useEffect(() => writePref("submarine-session-tree-side", side), [side]);

  // A new server in a folder (or at the top level): the Add server panel,
  // with that folder already picked.
  const addServerIn = (folderId?: number | null) => {
    o.newServer.setNewNode({
      ...o.newServer.defaultNode,
      folderId: folderId != null ? String(folderId) : "",
    });
    o.newServer.setFormError("");
    o.newServer.setIsPanelOpen(true);
  };

  // Rethrows so the session tree can say it failed; the grid ignores that.
  const cloneServer = async (id: number) => {
    try {
      const copyId = await invoke<number>("clone_server", { id });
      // An RDP server's window and sharing options (util/rdpOptions), and an
      // SSH server's keepalives and reconnecting (util/sessionKeep), are this
      // device's own, kept by sync id: the copy starts with the same.
      const original = (o.servers || []).find((s: any) => s.id === id);
      const isRdp = original?.protocol === "rdp";
      const isSsh = !["rdp", "telnet", "vnc"].includes(original?.protocol);
      if (original?.uuid && (isRdp || isSsh)) {
        try {
          const copy = (await invoke<any[]>("get_servers")).find((s) => s.id === copyId);
          if (copy?.uuid && isRdp) writeRdpOptions(copy.uuid, readRdpOptions(original.uuid));
          else if (copy?.uuid) writeSessionKeep(copy.uuid, readSessionKeep(original.uuid));
        } catch (e) {
          o.addLog(`CLONE_DEVICE_OPTIONS_FAILED: ${e}`, "error");
        }
      }
      o.refreshServers(); o.bumpSync(); o.addLog("Node cloned.", "success");
    }
    catch (e) { o.addLog(`CLONE_ERROR: ${e}`, "error"); throw e; }
  };

  // Rethrows: the tree says what failed itself, the grid logs it.
  const createFolder = async (name: string, parentId: number | null) => {
    await invoke("add_folder", { name, parentId });
    await o.refreshFolders();
    o.bumpSync();
    o.addLog(`Folder "${name}" created.`, "info");
  };

  // The session tree's drag and drop and "Move to…" (tree_moves.rs).
  // Rethrows so the tree can say what didn't move.
  const moveItems = async (items: TreeItems, folderId: number | null) => {
    try {
      if (items.servers.length) await invoke("move_servers", { ids: items.servers, folderId });
      for (const id of items.folders) await invoke("move_folder", { id, parentId: folderId });
    } finally {
      await Promise.all([o.refreshFolders(), o.refreshServers()]);
      o.bumpSync();
    }
  };

  // The session tree's Delete (tree_moves::delete_tree_items): one call and
  // one vault save however much is selected. Rethrows so the tree can say
  // what failed.
  const deleteItems = async (items: TreeItems) => {
    // The servers that go, the picked ones and those in the picked folders,
    // for the same per-server cleanup removeServer does.
    const tree = folderTree(o.folders);
    const insideFolders = new Set<number>(items.folders.flatMap((f) => [...tree.subtreeOf(f)]));
    const gone = new Set<number>([
      ...items.servers,
      ...(o.servers || []).filter((s: any) => insideFolders.has(tree.serverFolder(s) as number)).map((s: any) => s.id as number),
    ]);
    try {
      await invoke("delete_tree_items", { servers: items.servers, folders: items.folders });
      (o.servers || []).filter((s: any) => gone.has(s.id)).forEach(o.forgetServer);
      o.addLog("Removed from the session tree.", "info");
    } finally {
      await Promise.all([o.refreshFolders(), o.refreshServers()]);
      o.bumpSync();
    }
  };

  // Someone moved a server (in a shared profile, or in a profile file from
  // elsewhere) and this device hasn't allowed it: it still connects, but not
  // unasked (approvals::hold_moved). Cancel has the focus, as for held
  // commands. The servers that may open; one open already isn't asked about.
  const passMoved = async (list: any[]) => {
    const isOpen = new Set(o.sessions.map((s) => s.id));
    const ok: any[] = [];
    for (const server of list) {
      const moved: string | undefined = isOpen.has(`session-${server.id}`)
        ? undefined
        : (server.held || []).find((i: string) => i.startsWith("moved:"));
      if (moved) {
        const yes = await confirm({
          title: "Connect to the new address?",
          message: `${server.name} now connects to ${server.moved_to || "a new address"}. It connected to ${moved.slice("moved:".length)}, and this device hasn't allowed the change.`,
          okLabel: "Allow and connect",
          cancelLabel: "Not now",
          safeDefault: true,
        });
        if (!yes) continue;
        await o.allowHeld(server.id, [moved]);
      }
      ok.push(server);
    }
    return ok;
  };
  // Open servers, each in its own tab (one already open is just shown);
  // focus lands on the first. Several at once from the tree's multi-select
  // and "Open all".
  // The servers opened (those a teammate moved held back): none, or all of
  // `all` that are left.
  const openServers = async (all: any[]): Promise<any[]> => {
    const list = await passMoved(all);
    if (list.length === 0) return [];
    o.setSessions((prev) => {
      const seen = new Set(prev.map((p) => p.id));
      const fresh = list
        .filter((server) => !seen.has(`session-${server.id}`))
        .map(sessionFor);
      return [...prev, ...fresh];
    });
    o.setActiveView(`session-${list[0].id}`);
    return list;
  };
  // Broadcast to a folder, or the servers picked (#218): open them, and tick
  // them in Broadcast, which turns it on and lays them out on the Wall.
  const broadcast = useBroadcast();
  const broadcastServers = async (all: any[]) => {
    const list = await openServers(all);
    if (list.length < 2) return;
    broadcast.setTargets(list.map((server) => `session-${server.id}`));
    // Their tabs fold into Broadcast's (#224), even while it was on already.
    o.setActiveView("wall");
  };
  // Share on the AI's request for a server (OPEN_SERVER, #32): opened as a
  // click here would open it, a server a teammate moved held back as here.
  const opener = useRef({ openServers, servers: o.servers });
  opener.current = { openServers, servers: o.servers };
  useEffect(() => {
    const open = (e: Event) => {
      const id = (e as CustomEvent<unknown>).detail;
      const server = opener.current.servers.find((x) => x.id === id);
      if (server) void opener.current.openServers([server]);
    };
    window.addEventListener(OPEN_SERVER, open);
    // "Open terminal here" on a local folder (#72): Home, where it opens.
    const home = () => o.setActiveView("home");
    window.addEventListener(HOME_HERE, home);
    return () => {
      window.removeEventListener(OPEN_SERVER, open);
      window.removeEventListener(HOME_HERE, home);
    };
  }, []);

  return {
    open, side, setOpen,
    /** The grid's: a folder that can't be made is logged. */
    createFolder: (name: string, parentId: number | null) =>
      createFolder(name, parentId).catch((e) => { o.addLog(`Couldn't create the folder: ${e}`, "error"); throw e; }),
    /** The grid's: the log says what failed. */
    cloneServer: (id: number) => cloneServer(id).catch(() => {}),
    props: {
      profile: o.profile ?? "",
      servers: o.servers,
      folders: o.folders,
      sessions: o.sessions,
      statuses: o.statuses,
      waiting: o.waiting,
      activeSessionId: o.activeView,
      onOpenServers: (list: any[]) => void openServers(list),
      onBroadcastServers: (list: any[]) => void broadcastServers(list),
      onShowSession: (sid: string) => {
        o.setMergedSessionIds([]);
        o.setActiveView(sid);
      },
      onEditServer: o.editServer,
      onNewServer: addServerIn,
      onCloneServer: cloneServer,
      onCreateFolder: createFolder,
      onRenameFolder: o.renameFolder,
      onDelete: deleteItems,
      onMove: moveItems,
      side,
      onToggleSide: () => setSide((s) => (s === "left" ? "right" : "left")),
      onClose: () => setOpen(false),
    },
  };
};

export type SessionTreeState = ReturnType<typeof useSessionTree>;

import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  Activity, ArrowLeftRight, Bot, ChevronRight, ChevronsDownUp, Copy, CopyPlus, Folder, FolderInput, FolderOpen, FolderPlus,
  Eye, LayoutGrid, Monitor, MonitorSmartphone, Pencil, Play, Plus, Search, ShieldAlert, TerminalSquare, Trash2, X,
} from "lucide-react";
import { useConfirm, useTextPrompt } from "../ui/confirm";
import { FolderRow, indexFolders, subtreeCounts, subtreeIds } from "../util/folderPath";
import { sessionDot } from "../util/sessionStatus";
import RdpLinkModal, { type RdpLink } from "./RdpLinkModal";
import MoveToFolderModal from "./MoveToFolderModal";
import { OsBadge, osName } from "./OsBadge";
import { openAiTrust, useAi } from "../util/ai";
import { pingServers } from "../ui/ping";

// The session tree beside the main view, in the spirit of MobaXterm's
// Sessions sidebar: every folder, server and Remote Desktop link as one
// compact list, with the servers that have a tab open marked by their
// connection state. Double-click (or Enter) opens; Ctrl/Shift-click picks
// several to open together; a folder's menu opens everything in it. The
// arrow keys move through the rows, and → / ← open and close a folder.
//
// It's where the tree is managed too: right-click for new folders, servers
// and links in a folder, rename, duplicate, move and delete; drag rows onto a
// folder (or the empty space below the rows, for the top level) to move them,
// several at once when they're selected; F2 renames a folder, Delete deletes.

type Row =
  | { kind: "folder"; id: number; name: string; depth: number; open: boolean; count: number }
  | { kind: "server"; key: string; server: any; depth: number }
  | { kind: "rdp"; key: string; link: RdpLink; depth: number };

// What a move or a delete is about, by id.
export type TreeItems = { servers: number[]; links: number[]; folders: number[] };

// `row` null: a right-click on the empty space below the rows.
type Menu = { x: number; y: number; row: Row | null };

// A drag in progress: what's carried, where the pointer is, and where it
// would land (a folder id, null for the top level, undefined for nowhere).
type Drag = { items: TreeItems; label: string; x: number; y: number; target: number | null | undefined };

// Which folders are open, per device like the other preferences and per
// profile, since folder ids are only unique within one vault. The tree works
// without it.
const expandedKey = (profile: string) => `submarine-session-tree-expanded-${profile}`;
const readExpanded = (profile: string): Set<number> => {
  try {
    const raw = localStorage.getItem(expandedKey(profile));
    return new Set(raw ? (JSON.parse(raw) as number[]) : []);
  } catch {
    return new Set();
  }
};

// "Show only open" is a per-device preference too, shared by profiles: it
// says how you like to look at the tree, not which folders exist.
const OPEN_ONLY_KEY = "submarine-session-tree-open-only";
const readOpenOnly = (): boolean => {
  try { return localStorage.getItem(OPEN_ONLY_KEY) === "1"; } catch { return false; }
};

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });

// How long "Moved … · Undo" stays under the tree.
const UNDO_MS = 15000;

// More than this many sessions at once asks first: each one connects.
const OPEN_MANY = 10;
// A wall is one tab however many it holds; past this many it still asks,
// as each one connects.
const WALL_MANY = 30;
// How far the pointer travels before a press becomes a drag, how long a drag
// hovers on a closed folder before it opens, and the band at the list's top
// and bottom edges that scrolls it.
const DRAG_START_PX = 8;
const DRAG_OPEN_MS = 700;
const DRAG_SCROLL_PX = 28;

// A server that isn't SSH says what it is, as in the server list; a Remote
// Desktop link says it's a link, so the two aren't taken for each other.
const BADGE = "shrink-0 px-1 rounded text-[8.5px] font-black tracking-wider border";
const SERVER_TYPES: Record<string, { label: string; title: string; tone: string }> = {
  rdp: { label: "RDP", title: "RDP server", tone: "bg-sky-500/10 text-sky-300 border-sky-500/20" },
  telnet: { label: "TELNET", title: "Telnet server (not encrypted)", tone: "bg-amber-500/10 text-amber-300 border-amber-500/20" },
  vnc: { label: "VNC", title: "VNC server", tone: "bg-violet-500/10 text-violet-300 border-violet-500/20" },
};

const errText = (e: any) => (typeof e === "string" ? e : (e?.message || String(e)));
// A Wall holds SSH terminals only: RDP, telnet and VNC servers are left off
// it, so they aren't counted for one either.
const wallServers = (list: any[]) => list.filter((s) => (s?.protocol ?? "ssh") === "ssh");
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export const SessionTree = ({
  profile, servers, folders, rdpLinks, sessions, statuses, waiting, activeSessionId,
  onOpenServers, onShowSession, onOpenAsWall, onOpenRdp, onEditServer, onNewServer, onCloneServer,
  onCreateFolder, onRenameFolder, onSaveRdp, onDelete, onMove,
  side, onToggleSide, onClose,
}: {
  profile: string;
  servers: any[];
  folders: FolderRow[];
  rdpLinks: RdpLink[];
  sessions: { id: string; serverId: number }[];
  statuses: Record<string, string>;
  waiting: Set<string>;
  activeSessionId: string;
  onOpenServers: (servers: any[]) => void;
  // Go to a server that's already open: its tab, or the Wall it's on.
  onShowSession: (sessionId: string) => void;
  // Several servers on one Wall tab instead of a tab each (WallView).
  onOpenAsWall: (servers: any[], name: string) => void;
  onOpenRdp: (id: number) => Promise<void>;
  onEditServer: (server: any) => void;
  onNewServer: (folderId: number | null) => void;
  onCloneServer: (id: number) => Promise<void>;
  onCreateFolder: (name: string, parentId: number | null) => Promise<void>;
  onRenameFolder: (id: number, name: string) => Promise<void>;
  onSaveRdp: (link: RdpLink) => Promise<void>;
  // Everything given, folders with what's in them (tree_moves.rs).
  onDelete: (items: TreeItems) => Promise<void>;
  onMove: (items: TreeItems, folderId: number | null) => Promise<void>;
  side: "left" | "right";
  onToggleSide: () => void;
  onClose: () => void;
}) => {
  const confirm = useConfirm();
  const prompt = useTextPrompt();
  // "AI approval…" in the menus, while AI tools are on (#115).
  const aiOn = useAi().enabled;
  const [search, setSearch] = useState("");
  const [openOnly, setOpenOnly] = useState(readOpenOnly);
  const [expanded, setExpanded] = useState<Set<number>>(() => readExpanded(profile));
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rdpDraft, setRdpDraft] = useState<RdpLink | null>(null);
  const [moving, setMoving] = useState<TreeItems | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  // The last move, to put back: where each moved thing was before.
  const [undo, setUndo] = useState<{ text: string; back: { items: TreeItems; folderId: number | null }[] } | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!undo) return;
    const t = window.setTimeout(() => setUndo(null), UNDO_MS);
    return () => window.clearTimeout(t);
  }, [undo]);

  useEffect(() => {
    try { localStorage.setItem(expandedKey(profile), JSON.stringify([...expanded])); } catch { /* per-device nicety only */ }
  }, [expanded, profile]);

  useEffect(() => {
    try { localStorage.setItem(OPEN_ONLY_KEY, openOnly ? "1" : "0"); } catch { /* per-device nicety only */ }
  }, [openOnly]);

  const index = useMemo(() => indexFolders(folders || []), [folders]);
  const known = (id: number | null | undefined) => (id != null && index.byId.has(id) ? id : null);

  // Servers and links grouped by the folder they show in, sorted by name.
  const contents = useMemo(() => {
    const m = new Map<number | null, { servers: any[]; links: RdpLink[] }>();
    const at = (id: number | null) => {
      if (!m.has(id)) m.set(id, { servers: [], links: [] });
      return m.get(id)!;
    };
    for (const s of servers || []) at(known(s.folder_id)).servers.push(s);
    for (const l of rdpLinks || []) at(known(l.folder_id)).links.push(l);
    for (const v of m.values()) {
      v.servers.sort((a, b) => byName(a.name, b.name));
      v.links.sort((a, b) => byName(a.name, b.name));
    }
    return m;
  }, [servers, rdpLinks, index]);

  // Everything inside each folder, at any depth, for its count.
  const counts = useMemo(
    () => subtreeCounts(index, [...(servers || []).map((s) => s.folder_id), ...(rdpLinks || []).map((l) => l.folder_id)]),
    [servers, rdpLinks, index],
  );

  const q = search.trim().toLowerCase();
  const matchesServer = (s: any) => s.name.toLowerCase().includes(q) || String(s.host).toLowerCase().includes(q);
  const matchesLink = (l: RdpLink) => l.name.toLowerCase().includes(q) || l.host.toLowerCase().includes(q);
  const matchesFolder = (name: string) => name.toLowerCase().includes(q);
  // Servers with a session open (a tab, or a tile on a Wall).
  const openIds = useMemo(() => new Set((sessions || []).map((x) => x.serverId)), [sessions]);
  // Searching or showing only what's open lists matches with their folders
  // all open, so folders can't be opened or closed meanwhile.
  const flat = !!q || openOnly;

  // The rows on screen, top to bottom. While searching, only matches and
  // the folders leading to them, all open; a folder whose name matches
  // shows everything in it. With "show only open", only servers that are
  // open (no links, which have no tab) and the folders holding them.
  const rows = useMemo(() => {
    const out: Row[] = [];
    const hits = new Map<number | null, boolean>();
    const live = new Map<number | null, boolean>();
    // A folder holds an open server, at any depth.
    const hasOpen = (id: number | null): boolean => {
      if (live.has(id)) return live.get(id)!;
      const r = !!contents.get(id)?.servers.some((s) => openIds.has(s.id)) || (index.children.get(id) ?? []).some((f) => hasOpen(f.id));
      live.set(id, r);
      return r;
    };
    const serverOk = (s: any) => !openOnly || openIds.has(s.id);
    const hasHit = (id: number | null): boolean => {
      if (hits.has(id)) return hits.get(id)!;
      const c = contents.get(id);
      const own = !!c && (c.servers.some((s) => serverOk(s) && matchesServer(s)) || (!openOnly && c.links.some(matchesLink)));
      // A subfolder counts only if it shows: with "show only open", one with
      // nothing open is left out, name match or not.
      const r = own || (index.children.get(id) ?? []).some((f) => (!openOnly || hasOpen(f.id)) && (matchesFolder(f.name) || hasHit(f.id)));
      hits.set(id, r);
      return r;
    };
    // `all`: inside a folder whose name matched, so everything shows.
    const walk = (parent: number | null, depth: number, all: boolean) => {
      for (const f of index.children.get(parent) ?? []) {
        if (openOnly && !hasOpen(f.id)) continue;
        const named = !!q && matchesFolder(f.name);
        if (q && !all && !named && !hasHit(f.id)) continue;
        const open = flat ? true : expanded.has(f.id);
        out.push({ kind: "folder", id: f.id, name: f.name, depth, open, count: counts.get(f.id) ?? 0 });
        if (open) walk(f.id, depth + 1, all || named);
      }
      const c = contents.get(parent);
      for (const s of c?.servers ?? []) {
        if (serverOk(s) && (!q || all || matchesServer(s))) out.push({ kind: "server", key: `s:${s.id}`, server: s, depth });
      }
      for (const l of c?.links ?? []) {
        if (!openOnly && (!q || all || matchesLink(l))) out.push({ kind: "rdp", key: `r:${l.id}`, link: l, depth });
      }
    };
    walk(null, 0, false);
    return out;
  }, [index, contents, counts, expanded, q, openOnly, openIds]);

  const toggleFolder = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  const openFolder = (id: number) => {
    if (flat) return; // while searching or showing only open, every folder shows open anyway
    setExpanded((prev) => (prev.has(id) ? prev : new Set(prev).add(id)));
  };

  const rowKey = (r: Row) => (r.kind === "folder" ? `f:${r.id}` : r.key);
  // The folder a row sits in, or is (for a folder): where "new … here" goes.
  const folderOfRow = (r: Row | null | undefined): number | null =>
    !r ? null : r.kind === "folder" ? r.id : r.kind === "server" ? known(r.server.folder_id) : known(r.link.folder_id);

  // The selection is what's on screen: rows hidden by a search or a closed
  // folder, or deleted, drop out of it, so "Open N selected" opens what it
  // says.
  useEffect(() => {
    const shown = new Set(rows.map(rowKey));
    setSelected((prev) => {
      const next = new Set([...prev].filter((k) => shown.has(k)));
      return next.size === prev.size ? prev : next;
    });
    setAnchor((a) => (a && !shown.has(a) ? null : a));
  }, [rows]);

  // Keep the row the keyboard moved to in view.
  useEffect(() => {
    if (!anchor) return;
    listRef.current?.querySelector(`[data-key="${anchor}"]`)?.scrollIntoView({ block: "nearest" });
  }, [anchor]);

  const select = (r: Row, e: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => {
    const key = rowKey(r);
    if (e.shiftKey && anchor) {
      const keys = rows.filter((x) => x.kind !== "folder").map(rowKey);
      const a = keys.indexOf(anchor);
      const b = keys.indexOf(key);
      if (a >= 0 && b >= 0) {
        setSelected(new Set(keys.slice(Math.min(a, b), Math.max(a, b) + 1)));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      setSelected((prev) => {
        const next = new Set(prev);
        if (next.has(key)) next.delete(key); else next.add(key);
        return next;
      });
    } else {
      setSelected(new Set([key]));
    }
    setAnchor(key);
  };

  const openRdp = async (l: RdpLink) => {
    setError(null);
    try { await onOpenRdp(l.id as number); }
    catch (e: any) { setError(`${l.name}: ${errText(e)}`); }
  };

  // A double-click or Enter on a server. With "show only open" the tree is
  // for getting around, so an open one is shown where it is (its tab, or its
  // Wall) rather than pulled out of its Wall into a tab of its own.
  const openServer = (server: any) => {
    const open = openOnly ? sessions.find((x) => x.serverId === server.id) : undefined;
    if (open) onShowSession(open.id); else onOpenServers([server]);
  };

  // Open servers (tabs) and links together, asking first for a big batch.
  const openMany = async (list: any[], links: RdpLink[]) => {
    const n = list.length + links.length;
    if (n === 0) return;
    if (n > OPEN_MANY && !(await confirm({
      title: `Open ${n} sessions?`,
      message: `Each one connects as it opens, in a tab of its own${links.length ? `, and each Remote Desktop link opens its own window` : ""}. "Open as wall" puts servers on one tab instead.`,
      okLabel: `Open ${n}`,
    }))) return;
    if (list.length === 1 && links.length === 0) openServer(list[0]);
    else if (list.length) onOpenServers(list);
    for (const l of links) await openRdp(l);
  };

  const pickedItems = (keys: Set<string>) => {
    const list: any[] = [];
    const links: RdpLink[] = [];
    for (const r of rows) {
      if (r.kind === "server" && keys.has(r.key)) list.push(r.server);
      if (r.kind === "rdp" && keys.has(r.key)) links.push(r.link);
    }
    return { list, links };
  };

  const picked = useMemo(() => pickedItems(selected), [selected, rows]);
  const pickedCount = picked.list.length + picked.links.length;
  const openSelected = () => openMany(picked.list, picked.links);
  const pickedWall = useMemo(() => wallServers(picked.list), [picked]);
  const openAsWall = async (all: any[], name: string) => {
    const list = wallServers(all);
    if (list.length === 0) return;
    if (list.length > WALL_MANY && !(await confirm({
      title: `Open ${list.length} servers as a wall?`,
      message: "Each one connects as it opens. Behind a jump host they log in a few at a time (Settings › Servers).",
      okLabel: `Open ${list.length}`,
    }))) return;
    onOpenAsWall(list, name);
  };
  const openSelectedAsWall = () => openAsWall(pickedWall, `${pickedWall.length} servers`);

  // The rows a move or a delete is about: the selection when `r` is in it
  // (or there is no `r`), else just `r`.
  // Something inside a selected folder isn't taken on its own: it goes with
  // its folder, which keeps a moved branch whole.
  const itemsFor = (r: Row | null): TreeItems => {
    const keys = !r || selected.has(rowKey(r)) ? selected : new Set([rowKey(r)]);
    const items: TreeItems = { servers: [], links: [], folders: [] };
    for (const k of keys) {
      const id = Number(k.slice(2));
      if (k.startsWith("s:")) items.servers.push(id);
      else if (k.startsWith("r:")) items.links.push(id);
      else if (k.startsWith("f:")) items.folders.push(id);
    }
    const inside = new Set(items.folders.flatMap((f) => subtreeIds(index, f).filter((x) => x !== f)));
    const covered = new Set([...inside, ...items.folders]);
    return {
      folders: items.folders.filter((f) => !inside.has(f)),
      servers: items.servers.filter((id) => { const f = known(servers.find((x) => x.id === id)?.folder_id); return f == null || !covered.has(f); }),
      links: items.links.filter((id) => { const f = known(rdpLinks.find((x) => x.id === id)?.folder_id); return f == null || !covered.has(f); }),
    };
  };
  const itemCount = (it: TreeItems) => it.servers.length + it.links.length + it.folders.length;
  const describe = (it: TreeItems) => {
    if (itemCount(it) === 1) {
      const s = it.servers[0] != null ? servers.find((x) => x.id === it.servers[0]) : null;
      const l = it.links[0] != null ? rdpLinks.find((x) => x.id === it.links[0]) : null;
      const f = it.folders[0] != null ? index.byId.get(it.folders[0]) : null;
      return `“${s?.name ?? l?.name ?? f?.name ?? "item"}”`;
    }
    return plural(itemCount(it), "item");
  };
  // Folders nothing being moved may go into: the folders moved, and
  // everything inside them.
  const excludedFor = (it: TreeItems) => new Set(it.folders.flatMap((f) => subtreeIds(index, f)));

  // Where each of `it` is now, grouped by folder: what Undo moves back to.
  const whereNow = (it: TreeItems) => {
    const groups = new Map<number | null, TreeItems>();
    const at = (f: number | null) => {
      if (!groups.has(f)) groups.set(f, { servers: [], links: [], folders: [] });
      return groups.get(f)!;
    };
    for (const id of it.servers) at(known(servers.find((x) => x.id === id)?.folder_id)).servers.push(id);
    for (const id of it.links) at(known(rdpLinks.find((x) => x.id === id)?.folder_id)).links.push(id);
    for (const id of it.folders) at(index.parent.get(id) ?? null).folders.push(id);
    return [...groups].map(([folderId, items]) => ({ items, folderId }));
  };

  const move = async (it: TreeItems, folderId: number | null) => {
    setError(null);
    if (folderId != null && excludedFor(it).has(folderId)) return;
    const back = whereNow(it);
    try {
      await onMove(it, folderId);
      if (folderId != null) openFolder(folderId);
      const to = folderId == null ? "the top level" : `“${index.byId.get(folderId)?.name ?? ""}”`;
      setUndo({ text: `Moved ${describe(it)} to ${to}`, back });
    } catch (e: any) { setError(`Couldn't move ${describe(it)}: ${errText(e)}`); }
  };

  const undoMove = async () => {
    const u = undo;
    setUndo(null);
    if (!u) return;
    try { for (const b of u.back) await onMove(b.items, b.folderId); }
    catch (e: any) { setError(`Couldn't put everything back: ${errText(e)}`); }
  };

  // Every server and link in a folder, at any depth.
  const folderItems = (id: number) => {
    const inside = subtreeIds(index, id).map((fid) => contents.get(fid));
    return {
      list: inside.flatMap((c) => c?.servers ?? []),
      links: inside.flatMap((c) => c?.links ?? []),
    };
  };

  const newFolder = async (parentId: number | null) => {
    const parent = parentId != null ? index.byId.get(parentId)?.name : null;
    const name = await prompt({
      title: "New folder",
      message: parent ? `Inside “${parent}”.` : "At the top level.",
      placeholder: "Folder name",
      okLabel: "Create",
      validate: (v) => (v.trim() ? null : "A folder needs a name."),
    });
    if (!name) return;
    setError(null);
    try {
      await onCreateFolder(name, parentId);
      if (parentId != null) openFolder(parentId);
    } catch (e: any) { setError(`Couldn't create “${name}”: ${errText(e)}`); }
  };

  const renameFolder = async (id: number) => {
    const current = index.byId.get(id)?.name ?? "";
    const name = await prompt({
      title: "Rename folder",
      initialValue: current,
      okLabel: "Rename",
      validate: (v) => (v.trim() ? null : "A folder needs a name."),
    });
    if (!name || name === current) return;
    setError(null);
    try { await onRenameFolder(id, name); }
    catch (e: any) { setError(`Couldn't rename “${current}”: ${errText(e)}`); }
  };

  const newRdp = (folderId: number | null) =>
    setRdpDraft({ name: "", host: "", port: 3389, username: "", folder_id: folderId, notes: "" });

  // Delete what `r` stands for (see itemsFor), once the user says so. What
  // goes is spelt out, counted the way the backend will delete it: a folder
  // takes every folder, server and link inside it.
  const remove = async (r: Row | null) => {
    const it = itemsFor(r);
    if (itemCount(it) === 0) return;
    const folderIds = new Set(it.folders.flatMap((f) => subtreeIds(index, f)));
    const nServers = new Set([...it.servers, ...(servers || []).filter((x) => folderIds.has(known(x.folder_id) as number)).map((x) => x.id)]).size;
    const nLinks = new Set([...it.links, ...(rdpLinks || []).filter((x) => folderIds.has(known(x.folder_id) as number)).map((x) => x.id)]).size;
    const parts = [
      folderIds.size && plural(folderIds.size, "folder"),
      nServers && plural(nServers, "server"),
      nLinks && plural(nLinks, "Remote Desktop link"),
    ].filter(Boolean) as string[];
    const list = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0];
    const ok = await confirm({
      title: itemCount(it) === 1 ? `Delete ${describe(it)}?` : `Delete ${plural(itemCount(it), "item")}?`,
      message: `${list} will be removed from Submarine. Nothing changes on the servers themselves, and tabs already open stay connected until you close them.`,
      okLabel: "Delete",
      destructive: true,
    });
    if (!ok) return;
    setError(null);
    try { await onDelete(it); }
    catch (e: any) { setError(`Couldn't delete ${describe(it)}: ${errText(e)}`); }
  };

  const selectOnly = (r: Row) => {
    setSelected(new Set([rowKey(r)]));
    setAnchor(rowKey(r));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = anchor ? rows.findIndex((r) => rowKey(r) === anchor) : -1;
    const cur = at >= 0 ? rows[at] : undefined;
    if (e.key === "F2" && cur?.kind === "folder") { e.preventDefault(); renameFolder(cur.id); return; }
    if (e.key === "Delete" && selected.size > 0) { e.preventDefault(); remove(null); return; }
    // While searching (or showing only open) every folder shows open, so
    // opening or closing one would change only what shows afterwards.
    if (e.key === "Enter") {
      e.preventDefault();
      if (cur?.kind === "folder") { if (!flat) toggleFolder(cur.id); } else openSelected();
      return;
    }
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && cur?.kind === "folder") {
      e.preventDefault();
      if (!flat && cur.open !== (e.key === "ArrowRight")) toggleFolder(cur.id);
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const next = Math.max(0, Math.min(rows.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
    if (rows[next]) selectOnly(rows[next]);
  };

  // ---- Drag and drop -------------------------------------------------------
  // Pointer events rather than HTML drag and drop, as the server grid does:
  // on Windows the webview's own handling of files dropped from outside
  // keeps HTML drag events from reaching the page. A press becomes a drag
  // past DRAG_START_PX; the click that ends it is swallowed.
  const pressRef = useRef<{ x: number; y: number; row: Row } | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const swallowClickRef = useRef(false);
  const hoverRef = useRef<{ folder: number; since: number } | null>(null);
  dragRef.current = drag;
  const setDragNow = (d: Drag | null) => { dragRef.current = d; setDrag(d); };
  const cancelDrag = () => {
    pressRef.current = null;
    hoverRef.current = null;
    if (!dragRef.current) return;
    setDragNow(null);
    // The click that ends it isn't a click.
    swallowClickRef.current = true;
    setTimeout(() => { swallowClickRef.current = false; }, 0);
  };
  // A closed folder held under a drag opens after DRAG_OPEN_MS, to drop
  // deeper.
  const holdOpen = (target: number | null | undefined) => {
    const fid = typeof target === "number" ? target : null;
    if (fid != null && !expanded.has(fid)) {
      if (hoverRef.current?.folder !== fid) hoverRef.current = { folder: fid, since: Date.now() };
      else if (Date.now() - hoverRef.current.since > DRAG_OPEN_MS) { openFolder(fid); hoverRef.current = null; }
    } else {
      hoverRef.current = null;
    }
  };

  // Where the pointer would drop: a folder row is that folder; a server or
  // link row is the folder it's in; the empty space under the rows is the
  // top level; anywhere outside the list, nowhere.
  const dropTargetAt = (x: number, y: number, it: TreeItems): number | null | undefined => {
    const list = listRef.current;
    const el = document.elementFromPoint(x, y);
    if (!list || !el || !list.contains(el)) return undefined;
    const rowEl = (el as HTMLElement).closest<HTMLElement>("[data-key]");
    if (!rowEl) return null;
    const r = rows.find((x) => rowKey(x) === rowEl.dataset.key);
    if (!r) return undefined;
    const target = folderOfRow(r);
    return target != null && excludedFor(it).has(target) ? undefined : target;
  };

  useEffect(() => {
    const onMovePointer = (e: PointerEvent) => {
      const press = pressRef.current;
      if (!press) return;
      // The button came up somewhere this never heard of (outside the
      // window, say): nothing is being held any more.
      if ((e.buttons & 1) === 0) { cancelDrag(); return; }
      let d = dragRef.current;
      if (!d) {
        if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START_PX) return;
        const items = itemsFor(press.row);
        if (itemCount(items) === 0) return;
        d = { items, label: describe(items), x: e.clientX, y: e.clientY, target: undefined };
        // What's carried is what's selected: a row dragged on its own
        // becomes the selection, so it shows as picked up.
        if (!selected.has(rowKey(press.row))) selectOnly(press.row);
        setMenu(null);
      }
      const target = dropTargetAt(e.clientX, e.clientY, d.items);
      holdOpen(target);
      setDragNow({ ...d, x: e.clientX, y: e.clientY, target });
    };
    const onUp = (e: PointerEvent) => {
      const d = dragRef.current;
      if (!d) { pressRef.current = null; return; }
      // Where the pointer is now, not where the last move left it.
      const target = dropTargetAt(e.clientX, e.clientY, d.items);
      cancelDrag();
      if (target !== undefined) move(d.items, target);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && dragRef.current) cancelDrag();
    };
    // A touch scroll, a lost window, Alt+Tab: the drag is over, dropped
    // nowhere.
    const onCancel = () => cancelDrag();
    window.addEventListener("pointermove", onMovePointer);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("blur", onCancel);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointermove", onMovePointer);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("blur", onCancel);
      window.removeEventListener("keydown", onKey);
    };
  });

  // Scroll the list while a drag holds the pointer near its top or bottom.
  useEffect(() => {
    if (!drag) return;
    const timer = window.setInterval(() => {
      const d = dragRef.current;
      const list = listRef.current;
      if (!d || !list) return;
      const r = list.getBoundingClientRect();
      if (d.x < r.left || d.x > r.right) return;
      if (d.y < r.top + DRAG_SCROLL_PX) list.scrollTop -= 12;
      else if (d.y > r.bottom - DRAG_SCROLL_PX) list.scrollTop += 12;
      // The rows move under a still pointer, and a held folder opens.
      const target = dropTargetAt(d.x, d.y, d.items);
      holdOpen(target);
      if (target !== d.target) setDragNow({ ...d, target });
    }, 30);
    return () => window.clearInterval(timer);
  }, [!!drag]);

  const press = (r: Row) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    pressRef.current = { x: e.clientX, y: e.clientY, row: r };
  };
  // A click right after a drag is the drag's own pointer-up, not a click.
  const clicked = (f: (e: React.MouseEvent) => void) => (e: React.MouseEvent) => {
    if (swallowClickRef.current) return;
    f(e);
  };

  const statusOf = (serverId: number) => {
    const sess = sessions.find((s) => s.serverId === serverId);
    if (!sess) return null;
    return { id: sess.id, dot: sessionDot(statuses[sess.id] ?? "connecting", waiting.has(sess.id)) };
  };
  const menuItem = "w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left disabled:opacity-40 disabled:hover:bg-transparent";
  const menuIcon = "text-zinc-500";
  const menuSep = <div className="my-1 border-t border-white/5" />;
  const run = (f: () => void) => () => { setMenu(null); f(); };

  // The "new …" items, for a folder (named) or the top level.
  const newItems = (folderId: number | null) => {
    const where = folderId != null ? ` in “${index.byId.get(folderId)?.name ?? ""}”` : "";
    return (
      <>
        <button className={menuItem} onClick={run(() => newFolder(folderId))}>
          <FolderPlus size={13} className={menuIcon} /> <span className="truncate">New folder{where}</span>
        </button>
        <button className={menuItem} onClick={run(() => onNewServer(folderId))}>
          <Plus size={13} className={menuIcon} /> <span className="truncate">New server{where}</span>
        </button>
        <button className={menuItem} onClick={run(() => newRdp(folderId))}>
          <MonitorSmartphone size={13} className={menuIcon} /> <span className="truncate">New Remote Desktop link{where}</span>
        </button>
      </>
    );
  };

  return (
    <aside
      className={`w-72 shrink-0 h-full flex flex-col bg-background ${side === "left" ? "border-r" : "border-l"} border-white/5 relative z-[5]`}
    >
      <div className="shrink-0 px-2.5 pt-3 pb-2 space-y-2 border-b border-white/5">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Sessions</span>
          <div className="flex items-center gap-0.5">
            <button
              onClick={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                // At the top level, as a right-click on the empty space;
                // a folder's own menu makes things inside it.
                setMenu({ x: rect.left, y: rect.bottom + 4, row: null });
              }}
              title="New folder, server or Remote Desktop link at the top level"
              className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-white/5"
            >
              <Plus size={13} />
            </button>
            <button
              onClick={() => setOpenOnly((v) => !v)}
              aria-pressed={openOnly}
              title={openOnly ? "Showing only servers that are open — click to show all" : "Show only servers that are open"}
              className={`p-1 rounded hover:bg-white/5 ${openOnly ? "text-primary" : "text-zinc-500 hover:text-zinc-200"}`}
            >
              <Eye size={13} />
            </button>
            <button onClick={() => setExpanded(new Set())} title="Collapse all folders" className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-white/5"><ChevronsDownUp size={13} /></button>
            <button onClick={onToggleSide} title={`Move to the ${side === "left" ? "right" : "left"}`} className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-white/5"><ArrowLeftRight size={13} /></button>
            <button onClick={onClose} title="Hide the session tree" className="p-1 rounded text-zinc-500 hover:text-zinc-200 hover:bg-white/5"><X size={13} /></button>
          </div>
        </div>
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 text-zinc-500" size={12} />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setSearch("");
              // ↓ or Enter: on to the first match, to open with Enter. Not
              // the Enter that ends an IME composition.
              if (e.key === "ArrowDown" || (e.key === "Enter" && !e.nativeEvent.isComposing)) {
                e.preventDefault();
                const first = rows.find((r) => r.kind !== "folder") ?? rows[0];
                if (first) selectOnly(first);
                listRef.current?.focus();
              }
            }}
            placeholder="Find a session or folder…"
            className="w-full h-7 bg-[#1c1c21] border border-white/10 rounded-md pl-7 pr-2 text-[12px] text-zinc-100 outline-none focus:border-primary/50 placeholder:text-zinc-600"
          />
        </div>
        {pickedCount > 1 && (
          <button
            onClick={openSelected}
            className="w-full h-7 rounded-md text-[11px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25 flex items-center justify-center gap-1.5"
          >
            <Play size={11} /> Open {pickedCount} selected
          </button>
        )}
      </div>

      <div
        ref={listRef}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onContextMenu={(e) => {
          // On a row, the row's own handler has run and stopped this.
          e.preventDefault();
          if (dragRef.current) return;
          setMenu({ x: e.clientX, y: e.clientY, row: null });
        }}
        className={`flex-1 overflow-y-auto custom-scrollbar py-1 outline-none select-none ${
          drag?.target === null ? "ring-1 ring-inset ring-primary/40 bg-primary/[0.04]" : ""
        }`}
      >
        {rows.length === 0 && (
          <div className="px-3 py-6 text-center text-[11.5px] text-zinc-600 italic">
            {q ? "No matches." : openOnly ? "No server is open. Turn off “show only open” to see them all." : "No sessions yet — right-click for a new folder, server or link."}
          </div>
        )}
        {rows.map((r) => {
          const pad = { paddingLeft: 6 + r.depth * 14 };
          if (r.kind === "folder") {
            const key = rowKey(r);
            const isDrop = drag?.target === r.id;
            return (
              <div
                key={key}
                data-key={key}
                onPointerDown={press(r)}
                onClick={clicked((e) => {
                  if (e.ctrlKey || e.metaKey) { select(r, e); return; }
                  toggleFolder(r.id);
                  selectOnly(r);
                })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  if (dragRef.current) return;
                  if (!selected.has(key)) selectOnly(r);
                  setMenu({ x: e.clientX, y: e.clientY, row: r });
                }}
                style={pad}
                className={`h-[22px] pr-2 flex items-center gap-1 text-[12px] cursor-pointer ${
                  isDrop ? "bg-primary/30 text-white ring-1 ring-inset ring-primary/60"
                    : selected.has(key) ? "bg-primary/20 text-white" : "text-zinc-300 hover:bg-white/[0.04]"
                }`}
                title={`${r.name} — ${r.count} inside`}
              >
                <ChevronRight size={12} className={`shrink-0 text-zinc-500 transition-transform ${r.open ? "rotate-90" : ""}`} />
                {r.open ? <FolderOpen size={13} className="shrink-0 text-indigo-300" /> : <Folder size={13} className="shrink-0 text-indigo-300" />}
                <span className="truncate">{r.name}</span>
                <span className="ml-auto pl-2 text-[10px] text-zinc-600 font-mono">{r.count}</span>
              </div>
            );
          }
          const isSel = selected.has(r.key);
          const isServer = r.kind === "server";
          const name = isServer ? r.server.name : r.link.name;
          const host = isServer ? r.server.host : r.link.host;
          const status = isServer ? statusOf(r.server.id) : null;
          const isActive = !!status && status.id === activeSessionId;
          const isCarried = !!drag && isSel;
          const type = isServer ? SERVER_TYPES[r.server.protocol ?? ""] : undefined;
          return (
            <div
              key={r.key}
              data-key={r.key}
              onPointerDown={press(r)}
              onClick={clicked((e) => select(r, e))}
              onDoubleClick={() => (isServer ? openServer(r.server) : openRdp(r.link))}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (dragRef.current) return;
                if (!selected.has(r.key)) selectOnly(r);
                setMenu({ x: e.clientX, y: e.clientY, row: r });
              }}
              style={{ paddingLeft: 6 + r.depth * 14 + 12 }}
              className={`h-[22px] pr-2 flex items-center gap-1.5 text-[12px] cursor-pointer ${
                isSel ? "bg-primary/20 text-white" : isActive ? "bg-white/[0.06] text-white" : "text-zinc-300 hover:bg-white/[0.04]"
              } ${isCarried ? "opacity-50" : ""}`}
              title={`${name}\n${host}${isServer ? (type ? ` · ${type.title}` : r.server.os ? ` · ${osName(r.server.os)}` : "") : " · Remote Desktop link"}`}
            >
              {!isServer
                ? <MonitorSmartphone size={13} className="shrink-0 text-sky-300" />
                : type && r.server.protocol !== "telnet"
                  ? <Monitor size={13} className={`shrink-0 ${r.server.protocol === "vnc" ? "text-violet-300" : "text-sky-300"}`} />
                  : r.server.os && !type
                    ? <OsBadge os={r.server.os} size={13} />
                    : <TerminalSquare size={13} className="shrink-0 text-amber-300/90" />}
              <span className={`truncate ${isServer ? "" : "text-sky-200"}`}>{name}</span>
              {isServer && r.server.protocol === "telnet" && (
                <span title="Not encrypted: what is typed, passwords too, crosses the network as plain text." className="shrink-0 text-amber-300">
                  <ShieldAlert size={11} aria-label="Not encrypted" />
                </span>
              )}
              {!isServer ? (
                <span title="Remote Desktop link" className={`ml-auto ${BADGE} bg-sky-500/15 text-sky-300 border-sky-500/30`}>LINK</span>
              ) : type && (
                <span title={type.title} className={`ml-auto ${BADGE} ${type.tone}`}>{type.label}</span>
              )}
              {status && (
                <span title={status.dot.title} className={`${type ? "ml-1" : "ml-auto"} shrink-0 h-1.5 w-1.5 rounded-full ${status.dot.tone}`} />
              )}
            </div>
          );
        })}
        {/* Room under the rows to drop onto, for the top level. */}
        {rows.length > 0 && <div className="h-10" />}
      </div>

      {undo && (
        <div className="shrink-0 m-2 mb-0 px-2.5 py-1.5 bg-[#15151a] border border-white/10 rounded-md text-zinc-300 text-[11px] flex items-center gap-2">
          <span className="flex-1 truncate" title={undo.text}>{undo.text}</span>
          <button onClick={undoMove} className="shrink-0 px-2 h-6 rounded text-[10.5px] font-bold uppercase tracking-wider text-primary hover:bg-primary/15">Undo</button>
          <button onClick={() => setUndo(null)} className="shrink-0 text-zinc-500 hover:text-white"><X size={12} /></button>
        </div>
      )}

      {error && (
        <div className="shrink-0 m-2 px-2.5 py-2 bg-rose-500/15 border border-rose-500/30 rounded-md text-rose-200 text-[11px] flex items-start gap-2">
          <span className="flex-1">{error}</span>
          <button onClick={() => setError(null)} className="text-rose-200/70 hover:text-white"><X size={12} /></button>
        </div>
      )}

      {/* What a drag carries, and where it would land. */}
      {drag && createPortal(
        <div
          className="fixed z-[9999] pointer-events-none px-2 py-1 rounded-md bg-[#15151a] border border-primary/40 text-[11px] text-zinc-100 shadow-2xl flex items-center gap-1.5"
          style={{ left: drag.x + 14, top: drag.y + 10 }}
        >
          <FolderInput size={12} className="text-primary" />
          <span>
            Move {drag.label}
            {drag.target === undefined ? "" : ` to ${drag.target === null ? "the top level" : `“${index.byId.get(drag.target)?.name ?? ""}”`}`}
          </span>
        </div>,
        document.body,
      )}

      {/* A layer under the menu takes the click (or right-click) that closes
          it, so that click doesn't also select the row beneath. */}
      {menu && createPortal(
        <>
        <div
          className="fixed inset-0 z-[9998]"
          onClick={() => setMenu(null)}
          onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}
        />
        <div
          className="fixed z-[9999] w-[250px] py-1 bg-[#15151a] border border-white/10 rounded-lg shadow-2xl text-[11.5px]"
          style={{ left: Math.min(menu.x, window.innerWidth - 270), top: Math.min(menu.y, window.innerHeight - 360) }}
        >
          {menu.row === null ? (
            newItems(null)
          ) : menu.row.kind === "folder" ? (() => {
            const f = menu.row;
            const { list, links } = folderItems(f.id);
            const many = itemCount(itemsFor(f)) > 1;
            return (
              <>
                <button className={menuItem} onClick={run(() => openMany(list, links))} disabled={list.length + links.length === 0}>
                  <Play size={13} className={menuIcon} /> Open all in this folder ({list.length + links.length})
                </button>
                {wallServers(list).length > 0 && (
                  <button className={menuItem} onClick={run(() => openAsWall(list, f.name))}>
                    <LayoutGrid size={13} className={menuIcon} /> Open as wall ({wallServers(list).length})
                  </button>
                )}
                <button className={menuItem} onClick={run(() => toggleFolder(f.id))}>
                  <ChevronRight size={13} className={menuIcon} /> {f.open ? "Collapse" : "Expand"}
                </button>
                {aiOn && !many && (
                  <button className={menuItem} onClick={run(() => openAiTrust({ scope: "folder", id: f.id, name: f.name }))}>
                    <Bot size={13} className={menuIcon} /> AI approval for this folder…
                  </button>
                )}
                {menuSep}
                {newItems(f.id)}
                {menuSep}
                {!many && (
                  <button className={menuItem} onClick={run(() => renameFolder(f.id))}>
                    <Pencil size={13} className={menuIcon} /> Rename…<span className="ml-auto text-[10px] text-zinc-600">F2</span>
                  </button>
                )}
                <button className={menuItem} onClick={run(() => setMoving(itemsFor(f)))}>
                  <FolderInput size={13} className={menuIcon} /> Move{many ? ` ${itemCount(itemsFor(f))} selected` : ""} to…
                </button>
                <button className={`${menuItem} hover:!text-rose-300`} onClick={run(() => remove(f))}>
                  <Trash2 size={13} className={menuIcon} /> Delete{many ? ` ${itemCount(itemsFor(f))} selected` : " folder"}…<span className="ml-auto text-[10px] text-zinc-600">Del</span>
                </button>
              </>
            );
          })() : (() => {
            const r = menu.row;
            const many = itemCount(itemsFor(r)) > 1;
            const host = r.kind === "server" ? r.server.host : r.link.host;
            return (
              <>
                <button className={menuItem} onClick={run(() => (pickedCount > 1 && selected.has(r.key) ? openSelected() : r.kind === "server" ? openServer(r.server) : openRdp(r.link)))}>
                  <Play size={13} className={menuIcon} /> {pickedCount > 1 && selected.has(r.key) ? `Open ${pickedCount} selected` : r.kind === "server" ? "Open" : "Open in Remote Desktop"}
                </button>
                {pickedCount > 1 && selected.has(r.key) && pickedWall.length > 1 && (
                  <button className={menuItem} onClick={run(openSelectedAsWall)}>
                    <LayoutGrid size={13} className={menuIcon} /> Open {pickedWall.length} servers as a wall
                  </button>
                )}
                {!many && (
                  <button className={menuItem} onClick={run(() => (r.kind === "server" ? onEditServer(r.server) : setRdpDraft({ ...r.link })))}>
                    <Pencil size={13} className={menuIcon} /> Edit…
                  </button>
                )}
                {!many && (
                  <button
                    className={menuItem}
                    onClick={run(async () => {
                      setError(null);
                      try {
                        if (r.kind === "server") await onCloneServer(r.server.id);
                        else await onSaveRdp({ ...r.link, id: null, name: `${r.link.name} (copy)` });
                      } catch (e: any) { setError(`Couldn't duplicate “${r.kind === "server" ? r.server.name : r.link.name}”: ${errText(e)}`); }
                    })}
                  >
                    <CopyPlus size={13} className={menuIcon} /> Duplicate
                  </button>
                )}
                {r.kind === "server" && (() => {
                  // Several selected: each server among them.
                  const list = pickedCount > 1 && selected.has(r.key) ? picked.list : [r.server];
                  return list.length > 0 && (
                    <button className={menuItem} onClick={run(() => pingServers(list))}>
                      <Activity size={13} className={menuIcon} /> {list.length > 1 ? `Ping ${list.length} servers` : "Ping"}
                    </button>
                  );
                })()}
                {!many && (
                  <button className={menuItem} onClick={run(() => { navigator.clipboard?.writeText(String(host)).catch(() => {}); })}>
                    <Copy size={13} className={menuIcon} /> Copy host name
                  </button>
                )}
                {aiOn && !many && r.kind === "server" && (r.server.protocol ?? "ssh") === "ssh" && (
                  <button className={menuItem} onClick={run(() => openAiTrust({ scope: "server", id: r.server.id, name: r.server.name }))}>
                    <Bot size={13} className={menuIcon} /> AI approval…
                  </button>
                )}
                {menuSep}
                <button className={menuItem} onClick={run(() => setMoving(itemsFor(r)))}>
                  <FolderInput size={13} className={menuIcon} /> Move{many ? ` ${itemCount(itemsFor(r))} selected` : ""} to…
                </button>
                <button className={`${menuItem} hover:!text-rose-300`} onClick={run(() => remove(r))}>
                  <Trash2 size={13} className={menuIcon} /> Delete{many ? ` ${itemCount(itemsFor(r))} selected` : ""}…<span className="ml-auto text-[10px] text-zinc-600">Del</span>
                </button>
              </>
            );
          })()}
        </div>
        </>,
        document.body,
      )}

      {moving && createPortal(
        <MoveToFolderModal
          title={`Move ${describe(moving)} to…`}
          folders={folders}
          exclude={excludedFor(moving)}
          onPick={(folderId) => { const it = moving; setMoving(null); move(it, folderId); }}
          onClose={() => setMoving(null)}
        />,
        document.body,
      )}

      {rdpDraft && createPortal(
        <RdpLinkModal link={rdpDraft} folders={folders} onSave={onSaveRdp} onClose={() => setRdpDraft(null)} />,
        document.body,
      )}
    </aside>
  );
};

export default SessionTree;

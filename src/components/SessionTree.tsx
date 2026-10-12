import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  ArrowLeftRight, ChevronRight, ChevronsDownUp, Copy, Folder, FolderOpen, MonitorSmartphone,
  Pencil, Play, Search, TerminalSquare, X,
} from "lucide-react";
import { useConfirm } from "../ui/confirm";
import { FolderRow, indexFolders, subtreeCounts, subtreeIds } from "../util/folderPath";
import { sessionDot } from "../util/sessionStatus";
import type { RdpLink } from "./RdpLinkModal";
import { OsBadge, osName } from "./OsBadge";

// The session tree beside the main view, in the spirit of MobaXterm's
// Sessions sidebar: every folder, server and Remote Desktop link as one
// compact list, with the servers that have a tab open marked by their
// connection state. Double-click (or Enter) opens; Ctrl/Shift-click picks
// several to open together; a folder's menu opens everything in it. The
// arrow keys move through the rows, and → / ← open and close a folder.

type Row =
  | { kind: "folder"; id: number; name: string; depth: number; open: boolean; count: number }
  | { kind: "server"; key: string; server: any; depth: number }
  | { kind: "rdp"; key: string; link: RdpLink; depth: number };

type Menu = { x: number; y: number; row: Row };

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

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });

// More than this many sessions at once asks first: each one connects.
const OPEN_MANY = 10;

export const SessionTree = ({
  profile, servers, folders, rdpLinks, sessions, statuses, waiting, activeSessionId,
  onOpenServers, onOpenRdp, onEditServer, side, onToggleSide, onClose,
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
  onOpenRdp: (id: number) => Promise<void>;
  onEditServer: (server: any) => void;
  side: "left" | "right";
  onToggleSide: () => void;
  onClose: () => void;
}) => {
  const confirm = useConfirm();
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<Set<number>>(() => readExpanded(profile));
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    try { localStorage.setItem(expandedKey(profile), JSON.stringify([...expanded])); } catch { /* per-device nicety only */ }
  }, [expanded, profile]);

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

  // The rows on screen, top to bottom. While searching, only matches and
  // the folders leading to them, all open; a folder whose name matches
  // shows everything in it.
  const rows = useMemo(() => {
    const out: Row[] = [];
    const hits = new Map<number | null, boolean>();
    const hasHit = (id: number | null): boolean => {
      if (hits.has(id)) return hits.get(id)!;
      const c = contents.get(id);
      const own = !!c && (c.servers.some(matchesServer) || c.links.some(matchesLink));
      const r = own || (index.children.get(id) ?? []).some((f) => matchesFolder(f.name) || hasHit(f.id));
      hits.set(id, r);
      return r;
    };
    // `all`: inside a folder whose name matched, so everything shows.
    const walk = (parent: number | null, depth: number, all: boolean) => {
      for (const f of index.children.get(parent) ?? []) {
        const named = !!q && matchesFolder(f.name);
        if (q && !all && !named && !hasHit(f.id)) continue;
        const open = q ? true : expanded.has(f.id);
        out.push({ kind: "folder", id: f.id, name: f.name, depth, open, count: counts.get(f.id) ?? 0 });
        if (open) walk(f.id, depth + 1, all || named);
      }
      const c = contents.get(parent);
      for (const s of c?.servers ?? []) {
        if (!q || all || matchesServer(s)) out.push({ kind: "server", key: `s:${s.id}`, server: s, depth });
      }
      for (const l of c?.links ?? []) {
        if (!q || all || matchesLink(l)) out.push({ kind: "rdp", key: `r:${l.id}`, link: l, depth });
      }
    };
    walk(null, 0, false);
    return out;
  }, [index, contents, counts, expanded, q]);

  const toggleFolder = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });

  const rowKey = (r: Row) => (r.kind === "folder" ? `f:${r.id}` : r.key);

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
    catch (e: any) { setError(`${l.name}: ${typeof e === "string" ? e : (e?.message || String(e))}`); }
  };

  // Open servers (tabs) and links together, asking first for a big batch.
  const openMany = async (list: any[], links: RdpLink[]) => {
    const n = list.length + links.length;
    if (n === 0) return;
    if (n > OPEN_MANY && !(await confirm({
      title: `Open ${n} sessions?`,
      message: `Each one connects as it opens${links.length ? `, and each Remote Desktop link opens its own window` : ""}.`,
      okLabel: `Open ${n}`,
    }))) return;
    if (list.length) onOpenServers(list);
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

  // Every server and link in a folder, at any depth.
  const folderItems = (id: number) => {
    const inside = subtreeIds(index, id).map((fid) => contents.get(fid));
    return {
      list: inside.flatMap((c) => c?.servers ?? []),
      links: inside.flatMap((c) => c?.links ?? []),
    };
  };

  const selectOnly = (r: Row) => {
    setSelected(new Set([rowKey(r)]));
    setAnchor(rowKey(r));
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const at = anchor ? rows.findIndex((r) => rowKey(r) === anchor) : -1;
    const cur = at >= 0 ? rows[at] : undefined;
    // While searching every folder shows open, so opening or closing one
    // would change only what shows once the search is cleared.
    if (e.key === "Enter") {
      e.preventDefault();
      if (cur?.kind === "folder") { if (!q) toggleFolder(cur.id); } else openSelected();
      return;
    }
    if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && cur?.kind === "folder") {
      e.preventDefault();
      if (!q && cur.open !== (e.key === "ArrowRight")) toggleFolder(cur.id);
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const next = Math.max(0, Math.min(rows.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
    if (rows[next]) selectOnly(rows[next]);
  };

  const statusOf = (serverId: number) => {
    const sess = sessions.find((s) => s.serverId === serverId);
    if (!sess) return null;
    return { id: sess.id, dot: sessionDot(statuses[sess.id] ?? "connecting", waiting.has(sess.id)) };
  };
  const menuItem = "w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left disabled:opacity-40 disabled:hover:bg-transparent";

  return (
    <aside
      className={`w-72 shrink-0 h-full flex flex-col bg-background ${side === "left" ? "border-r" : "border-l"} border-white/5 relative z-[5]`}
    >
      <div className="shrink-0 px-2.5 pt-3 pb-2 space-y-2 border-b border-white/5">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Sessions</span>
          <div className="flex items-center gap-0.5">
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
        className="flex-1 overflow-y-auto custom-scrollbar py-1 outline-none select-none"
      >
        {rows.length === 0 && (
          <div className="px-3 py-6 text-center text-[11.5px] text-zinc-600 italic">
            {q ? "No matches." : "No sessions yet."}
          </div>
        )}
        {rows.map((r) => {
          const pad = { paddingLeft: 6 + r.depth * 14 };
          if (r.kind === "folder") {
            const key = rowKey(r);
            return (
              <div
                key={key}
                data-key={key}
                onClick={() => { toggleFolder(r.id); selectOnly(r); }}
                onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, row: r }); }}
                style={pad}
                className={`h-[22px] pr-2 flex items-center gap-1 text-[12px] cursor-pointer ${
                  selected.has(key) ? "bg-primary/20 text-white" : "text-zinc-300 hover:bg-white/[0.04]"
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
          return (
            <div
              key={r.key}
              data-key={r.key}
              onClick={(e) => select(r, e)}
              onDoubleClick={() => (isServer ? onOpenServers([r.server]) : openRdp(r.link))}
              onContextMenu={(e) => {
                e.preventDefault();
                if (!selected.has(r.key)) selectOnly(r);
                setMenu({ x: e.clientX, y: e.clientY, row: r });
              }}
              style={{ paddingLeft: 6 + r.depth * 14 + 12 }}
              className={`h-[22px] pr-2 flex items-center gap-1.5 text-[12px] cursor-pointer ${
                isSel ? "bg-primary/20 text-white" : isActive ? "bg-white/[0.06] text-white" : "text-zinc-300 hover:bg-white/[0.04]"
              }`}
              title={`${name}\n${host}${isServer ? (r.server.os ? ` · ${osName(r.server.os)}` : "") : " · Remote Desktop"}`}
            >
              {isServer
                ? (r.server.os ? <OsBadge os={r.server.os} size={13} /> : <TerminalSquare size={13} className="shrink-0 text-amber-300/90" />)
                : <MonitorSmartphone size={13} className="shrink-0 text-sky-300" />}
              <span className={`truncate ${isServer ? "" : "text-sky-200"}`}>{name}</span>
              {!isServer && (
                <span className="ml-auto shrink-0 px-1 rounded text-[8.5px] font-black tracking-wider bg-sky-500/15 text-sky-300 border border-sky-500/30">RDP</span>
              )}
              {status && (
                <span title={status.dot.title} className={`ml-auto shrink-0 h-1.5 w-1.5 rounded-full ${status.dot.tone}`} />
              )}
            </div>
          );
        })}
      </div>

      {error && (
        <div className="shrink-0 m-2 px-2.5 py-2 bg-rose-500/15 border border-rose-500/30 rounded-md text-rose-200 text-[11px] flex items-start gap-2">
          <span className="flex-1">{error}</span>
          <button onClick={() => setError(null)} className="text-rose-200/70 hover:text-white"><X size={12} /></button>
        </div>
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
          className="fixed z-[9999] w-[240px] py-1 bg-[#15151a] border border-white/10 rounded-lg shadow-2xl text-[11.5px]"
          style={{ left: Math.min(menu.x, window.innerWidth - 260), top: Math.min(menu.y, window.innerHeight - 160) }}
        >
          {menu.row.kind === "folder" ? (() => {
            const f = menu.row;
            const { list, links } = folderItems(f.id);
            return (
              <>
                <button className={menuItem} onClick={() => { setMenu(null); openMany(list, links); }} disabled={list.length + links.length === 0}>
                  <Play size={13} className="text-zinc-500" /> Open all in this folder ({list.length + links.length})
                </button>
                <button className={menuItem} onClick={() => { setMenu(null); toggleFolder(f.id); }}>
                  <ChevronRight size={13} className="text-zinc-500" /> {f.open ? "Collapse" : "Expand"}
                </button>
              </>
            );
          })() : (() => {
            const r = menu.row;
            const many = pickedCount > 1 && selected.has(r.key);
            const host = r.kind === "server" ? r.server.host : r.link.host;
            return (
              <>
                <button className={menuItem} onClick={() => { setMenu(null); many ? openSelected() : (r.kind === "server" ? onOpenServers([r.server]) : openRdp(r.link)); }}>
                  <Play size={13} className="text-zinc-500" /> {many ? `Open ${pickedCount} selected` : r.kind === "server" ? "Open" : "Open in Remote Desktop"}
                </button>
                {r.kind === "server" && !many && (
                  <button className={menuItem} onClick={() => { setMenu(null); onEditServer(r.server); }}>
                    <Pencil size={13} className="text-zinc-500" /> Edit
                  </button>
                )}
                {!many && (
                  <button className={menuItem} onClick={() => { setMenu(null); navigator.clipboard?.writeText(String(host)).catch(() => {}); }}>
                    <Copy size={13} className="text-zinc-500" /> Copy host name
                  </button>
                )}
              </>
            );
          })()}
        </div>
        </>,
        document.body,
      )}
    </aside>
  );
};

export default SessionTree;

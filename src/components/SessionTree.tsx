import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  ArrowLeftRight, ChevronRight, ChevronsDownUp, Copy, Folder, FolderOpen, MonitorSmartphone,
  Pencil, Play, Search, TerminalSquare, X,
} from "lucide-react";
import { useConfirm } from "../ui/confirm";
import { FolderRow, indexFolders } from "../util/folderPath";
import type { RdpLink } from "./RdpLinkModal";
import { OsBadge, osName } from "./OsBadge";

// The session tree beside the main view, in the spirit of MobaXterm's
// Sessions sidebar: every folder, server and Remote Desktop link as one
// compact list, with the servers that have a tab open marked by their
// connection state. Double-click (or Enter) opens; Ctrl/Shift-click picks
// several to open together; a folder's menu opens everything in it.

type Row =
  | { kind: "folder"; id: number; name: string; depth: number; open: boolean; count: number }
  | { kind: "server"; key: string; server: any; depth: number }
  | { kind: "rdp"; key: string; link: RdpLink; depth: number };

type Menu = { x: number; y: number; row: Row };

// Per-device view state; the tree works without it (see artifact-free note
// in SettingsPanel: preferences live in local storage).
const EXPANDED_KEY = "submarine.sessionTree.expanded";
const readExpanded = (): Set<number> => {
  try {
    const raw = localStorage.getItem(EXPANDED_KEY);
    return new Set(raw ? (JSON.parse(raw) as number[]) : []);
  } catch {
    return new Set();
  }
};

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });

// More than this many sessions at once asks first: each one connects.
const OPEN_MANY = 10;

export const SessionTree = ({
  servers, folders, rdpLinks, sessions, statuses, waiting, activeSessionId,
  onOpenServers, onOpenRdp, onEditServer, side, onToggleSide, onClose,
}: {
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
  const [expanded, setExpanded] = useState<Set<number>>(readExpanded);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [error, setError] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    try { localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded])); } catch { /* per-device nicety only */ }
  }, [expanded]);

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

  const q = search.trim().toLowerCase();
  const matchesServer = (s: any) => s.name.toLowerCase().includes(q) || String(s.host).toLowerCase().includes(q);
  const matchesLink = (l: RdpLink) => l.name.toLowerCase().includes(q) || l.host.toLowerCase().includes(q);

  // The rows on screen, top to bottom. While searching, only matches and
  // the folders leading to them, all open.
  const rows = useMemo(() => {
    const out: Row[] = [];
    const total = new Map<number, number>();
    const countOf = (id: number): number => {
      if (total.has(id)) return total.get(id)!;
      const c = contents.get(id);
      let n = (c?.servers.length ?? 0) + (c?.links.length ?? 0);
      for (const f of index.children.get(id) ?? []) n += countOf(f.id);
      total.set(id, n);
      return n;
    };
    const hits = new Map<number | null, boolean>();
    const hasHit = (id: number | null): boolean => {
      if (hits.has(id)) return hits.get(id)!;
      const c = contents.get(id);
      const own = !!c && (c.servers.some(matchesServer) || c.links.some(matchesLink));
      const r = own || (index.children.get(id) ?? []).some((f) => hasHit(f.id));
      hits.set(id, r);
      return r;
    };
    const walk = (parent: number | null, depth: number) => {
      for (const f of index.children.get(parent) ?? []) {
        if (q && !hasHit(f.id)) continue;
        const open = q ? true : expanded.has(f.id);
        out.push({ kind: "folder", id: f.id, name: f.name, depth, open, count: countOf(f.id) });
        if (open) walk(f.id, depth + 1);
      }
      const c = contents.get(parent);
      for (const s of c?.servers ?? []) {
        if (!q || matchesServer(s)) out.push({ kind: "server", key: `s:${s.id}`, server: s, depth });
      }
      for (const l of c?.links ?? []) {
        if (!q || matchesLink(l)) out.push({ kind: "rdp", key: `r:${l.id}`, link: l, depth });
      }
    };
    walk(null, 0);
    return out;
  }, [index, contents, expanded, q]);

  const toggleFolder = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });

  const rowKey = (r: Row) => (r.kind === "folder" ? `f:${r.id}` : r.key);

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

  const openSelected = () => {
    const { list, links } = pickedItems(selected);
    openMany(list, links);
  };

  // Every server and link in a folder, at any depth.
  const folderItems = (id: number) => {
    const list: any[] = [];
    const links: RdpLink[] = [];
    const walk = (fid: number) => {
      const c = contents.get(fid);
      list.push(...(c?.servers ?? []));
      links.push(...(c?.links ?? []));
      for (const f of index.children.get(fid) ?? []) walk(f.id);
    };
    walk(id);
    return { list, links };
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") { e.preventDefault(); openSelected(); return; }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const keys = rows.map(rowKey);
    const at = anchor ? keys.indexOf(anchor) : -1;
    const next = Math.max(0, Math.min(keys.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
    const r = rows[next];
    if (r) { setSelected(new Set([rowKey(r)])); setAnchor(rowKey(r)); }
  };

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => { window.removeEventListener("click", close); window.removeEventListener("blur", close); };
  }, [menu]);

  const statusOf = (serverId: number) => {
    const sess = sessions.find((s) => s.serverId === serverId);
    if (!sess) return null;
    const st = waiting.has(sess.id) ? "waiting" : (statuses[sess.id] ?? "connecting");
    return { id: sess.id, st };
  };
  const dot = (st: string) =>
    st === "waiting"      ? "bg-primary animate-pulse" :
    st === "connected"    ? "bg-emerald-400" :
    st === "connecting"   ? "bg-amber-400 animate-pulse" :
    st === "failed" || st === "disconnected" ? "bg-rose-500" : "bg-zinc-500";
  const dotTitle = (st: string) =>
    st === "waiting" ? "Waiting for you — open the tab to answer" :
    st === "connected" ? "Connected" : st === "connecting" ? "Connecting…" :
    st === "failed" ? "Connection failed" : st === "disconnected" ? "Disconnected" : st;

  const menuItem = "w-full flex items-center gap-2 px-3 py-1.5 text-left text-[12px] text-zinc-200 hover:bg-white/10";

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
            onKeyDown={(e) => { if (e.key === "Escape") setSearch(""); }}
            placeholder="Find a session…"
            className="w-full h-7 bg-[#1c1c21] border border-white/10 rounded-md pl-7 pr-2 text-[12px] text-zinc-100 outline-none focus:border-primary/50 placeholder:text-zinc-600"
          />
        </div>
        {selected.size > 1 && (
          <button
            onClick={openSelected}
            className="w-full h-7 rounded-md text-[11px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25 flex items-center justify-center gap-1.5"
          >
            <Play size={11} /> Open {selected.size} selected
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
            return (
              <div
                key={`f:${r.id}`}
                onClick={() => toggleFolder(r.id)}
                onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, row: r }); }}
                style={pad}
                className="h-[22px] pr-2 flex items-center gap-1 text-[12px] text-zinc-300 hover:bg-white/[0.04] cursor-pointer"
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
              onClick={(e) => select(r, e)}
              onDoubleClick={() => (isServer ? onOpenServers([r.server]) : openRdp(r.link))}
              onContextMenu={(e) => {
                e.preventDefault();
                if (!selected.has(r.key)) { setSelected(new Set([r.key])); setAnchor(r.key); }
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
                <span title={dotTitle(status.st)} className={`ml-auto shrink-0 h-1.5 w-1.5 rounded-full ${dot(status.st)}`} />
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

      {menu && createPortal(
        <div
          className="fixed z-[9999] min-w-[200px] py-1 bg-[#18181c] border border-white/10 rounded-lg shadow-2xl"
          style={{ left: Math.min(menu.x, window.innerWidth - 220), top: Math.min(menu.y, window.innerHeight - 160) }}
          onClick={(e) => e.stopPropagation()}
        >
          {menu.row.kind === "folder" ? (() => {
            const f = menu.row;
            const { list, links } = folderItems(f.id);
            return (
              <>
                <button className={menuItem} onClick={() => { setMenu(null); openMany(list, links); }} disabled={list.length + links.length === 0}>
                  <Play size={12} /> Open all in this folder ({list.length + links.length})
                </button>
                <button className={menuItem} onClick={() => { setMenu(null); toggleFolder(f.id); }}>
                  <ChevronRight size={12} /> {f.open ? "Collapse" : "Expand"}
                </button>
              </>
            );
          })() : (() => {
            const r = menu.row;
            const many = selected.size > 1 && selected.has(r.key);
            const host = r.kind === "server" ? r.server.host : r.link.host;
            return (
              <>
                <button className={menuItem} onClick={() => { setMenu(null); many ? openSelected() : (r.kind === "server" ? onOpenServers([r.server]) : openRdp(r.link)); }}>
                  <Play size={12} /> {many ? `Open ${selected.size} selected` : r.kind === "server" ? "Open" : "Open in Remote Desktop"}
                </button>
                {r.kind === "server" && !many && (
                  <button className={menuItem} onClick={() => { setMenu(null); onEditServer(r.server); }}>
                    <Pencil size={12} /> Edit
                  </button>
                )}
                {!many && (
                  <button className={menuItem} onClick={() => { setMenu(null); navigator.clipboard?.writeText(String(host)).catch(() => {}); }}>
                    <Copy size={12} /> Copy host name
                  </button>
                )}
              </>
            );
          })()}
        </div>,
        document.body,
      )}
    </aside>
  );
};

export default SessionTree;

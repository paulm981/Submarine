import { useEffect, useMemo, useState } from "react";
import {
  ExternalLink, LayoutGrid, Pencil, Plus, Radio, Search, TerminalSquare, Trash2, X,
} from "lucide-react";
import { folderPath, indexFolders, type FolderRow } from "../util/folderPath";
import { sessionDot } from "../util/sessionStatus";
import type { Wall, WallMember } from "./WallView";

// The title bar's menus for Walls (WallView): "Walls ▾" — the open ones and
// the saved ones — and "All tabs ▾", every open session in one searchable
// list (with a hundred open, the tab strip alone isn't a way to find one).
// Also the saved-wall editor and the picker that adds terminals to a Wall.

/// A saved wall (src-tauri/src/multiexec.rs): servers to open as a Wall.
export interface SavedWall { id: number | null; name: string; server_ids: number[]; cols: number; }

interface SessionRow { id: string; serverId: number; serverName: string; wallId?: string; }
interface ServerRow { id: number; name: string; host?: string; folder_id?: number | null; }

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const menuItem = "w-full flex items-center gap-2 px-2.5 py-1.5 rounded-md text-left text-[11.5px] transition-colors";
const iconBtn = "h-6 w-6 shrink-0 rounded flex items-center justify-center text-zinc-500 hover:bg-white/[0.08] transition-colors";

const Popover = ({ onDismiss, width, children }: { onDismiss: () => void; width: number; children: React.ReactNode }) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onDismiss(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDismiss]);
  return (
    <>
      <div className="fixed inset-0 z-[60]" onClick={onDismiss} onContextMenu={(e) => { e.preventDefault(); onDismiss(); }} />
      <div
        className="absolute z-[70] top-full mt-1 right-0 max-w-[calc(100vw-16px)] max-h-[75vh] flex flex-col bg-[#15151a] border border-white/10 rounded-lg shadow-2xl p-1.5"
        style={{ width }}
      >
        {children}
      </div>
    </>
  );
};

const Heading = ({ children }: { children: React.ReactNode }) => (
  <div className="px-2.5 pt-2 pb-1 text-[9.5px] font-bold uppercase tracking-widest text-zinc-500">{children}</div>
);

export const WallsMenu = ({
  walls, activeId, saved, canFromTabs,
  onShow, onCloseWall, onOpenSaved, onEditSaved, onDeleteSaved, onNew, onFromTabs, onDismiss,
}: {
  walls: Wall[];
  activeId: string;
  saved: SavedWall[];
  canFromTabs: boolean;
  onShow: (wallId: string) => void;
  onCloseWall: (wallId: string) => void;
  onOpenSaved: (w: SavedWall, broadcast: boolean) => void;
  onEditSaved: (w: SavedWall) => void;
  onDeleteSaved: (w: SavedWall) => void;
  onNew: () => void;
  onFromTabs: () => void;
  onDismiss: () => void;
}) => (
  <Popover onDismiss={onDismiss} width={320}>
    <div className="overflow-y-auto custom-scrollbar">
      {walls.length > 0 && (
        <>
          <Heading>Open</Heading>
          {walls.map((w) => (
            <div key={w.id} className="flex items-center gap-1">
              <button
                onClick={() => onShow(w.id)}
                className={`${menuItem} flex-1 min-w-0 ${w.id === activeId ? "bg-primary/15 text-primary" : "text-zinc-200 hover:bg-white/[0.06]"}`}
              >
                {w.broadcast ? <Radio size={12} className="shrink-0 text-orange-300" /> : <LayoutGrid size={12} className="shrink-0 text-zinc-500" />}
                <span className="truncate flex-1">{w.name}</span>
                <span className="text-[10px] text-zinc-500 tabular-nums">{w.members.length}</span>
              </button>
              <button onClick={() => onCloseWall(w.id)} title="Close this wall" className={`${iconBtn} hover:text-red-400`}>
                <X size={12} />
              </button>
            </div>
          ))}
        </>
      )}
      <Heading>Saved walls</Heading>
      {saved.length === 0 && (
        <div className="px-2.5 py-2 text-[11px] text-zinc-500 leading-relaxed">
          None yet. Save a wall from its bar, or make one here without connecting.
        </div>
      )}
      {saved.map((w) => (
        <div key={w.id} className="flex items-center gap-0.5 group">
          <button
            onClick={() => onOpenSaved(w, false)}
            title={`Open ${plural(w.server_ids.length, "server")} as a wall`}
            className={`${menuItem} flex-1 min-w-0 text-zinc-200 hover:bg-white/[0.06]`}
          >
            <LayoutGrid size={12} className="shrink-0 text-indigo-300" />
            <span className="truncate flex-1">{w.name}</span>
            <span className="text-[10px] text-zinc-500 tabular-nums">{w.server_ids.length}</span>
          </button>
          <button onClick={() => onOpenSaved(w, true)} title="Open and broadcast to this wall" className={`${iconBtn} hover:text-orange-300`}>
            <Radio size={12} />
          </button>
          <button onClick={() => onEditSaved(w)} title="Edit" className={`${iconBtn} hover:text-white`}>
            <Pencil size={11} />
          </button>
          <button onClick={() => onDeleteSaved(w)} title="Delete" className={`${iconBtn} hover:text-red-400`}>
            <Trash2 size={11} />
          </button>
        </div>
      ))}
      <div className="border-t border-white/5 my-1" />
      <button onClick={onNew} className={`${menuItem} text-zinc-200 hover:bg-white/[0.06]`}>
        <Plus size={12} className="text-zinc-500" /> New saved wall…
      </button>
      {canFromTabs && (
        <button onClick={onFromTabs} className={`${menuItem} text-zinc-200 hover:bg-white/[0.06]`}>
          <LayoutGrid size={12} className="text-zinc-500" /> Wall of the open tabs
        </button>
      )}
    </div>
  </Popover>
);

export const AllTabsMenu = ({
  sessions, walls, statuses, waiting, activeView, onShowSession, onShowWall, onCloseSessions, onDismiss,
}: {
  sessions: SessionRow[];
  walls: Wall[];
  statuses: Record<string, string>;
  waiting: Set<string>;
  activeView: string;
  /// Show it in its tab (one opened into a Wall gets a tab).
  onShowSession: (sessionId: string) => void;
  onShowWall: (wallId: string) => void;
  onCloseSessions: (ids: string[]) => void;
  onDismiss: () => void;
}) => {
  const [q, setQ] = useState("");
  const query = q.trim().toLowerCase();
  const shown = query ? sessions.filter((s) => s.serverName.toLowerCase().includes(query)) : sessions;
  const tabs = shown.filter((s) => !s.wallId);
  const inWalls = walls.map((w) => ({ wall: w, list: shown.filter((s) => s.wallId === w.id) })).filter((g) => g.list.length > 0);
  const dead = sessions.filter((s) => ["failed", "disconnected"].includes(statuses[s.id] ?? "connecting")).map((s) => s.id);
  // Enter goes to the highlighted match, the first until ↑/↓ move it: the
  // rows as listed, tabs first, then each wall's.
  const [hi, setHi] = useState(0);
  const order = [...tabs, ...inWalls.flatMap((g) => g.list)];
  const at = Math.min(hi, order.length - 1);
  const go = (s: SessionRow) => (s.wallId ? onShowWall(s.wallId) : onShowSession(s.id));

  const row = (s: SessionRow, walled: boolean) => {
    const dot = sessionDot(statuses[s.id] ?? "connecting", waiting.has(s.id));
    return (
      <div key={s.id} className="flex items-center gap-0.5">
        <button
          ref={(el) => { if (el && order[at]?.id === s.id) el.scrollIntoView({ block: "nearest" }); }}
          onClick={() => (walled && s.wallId ? onShowWall(s.wallId) : onShowSession(s.id))}
          title={walled ? "Show its wall" : "Show its tab"}
          className={`${menuItem} flex-1 min-w-0 ${activeView === s.id ? "bg-primary/15 text-primary" : "text-zinc-200 hover:bg-white/[0.06]"} ${order[at]?.id === s.id ? "ring-1 ring-inset ring-primary/40" : ""}`}
        >
          <span title={dot.title} className={`w-1.5 h-1.5 rounded-full shrink-0 ${dot.tone}`} />
          <span className="truncate flex-1">{s.serverName}</span>
        </button>
        {walled && (
          <button onClick={() => onShowSession(s.id)} title="Open in its own tab" className={`${iconBtn} hover:text-primary`}>
            <ExternalLink size={11} />
          </button>
        )}
        <button onClick={() => onCloseSessions([s.id])} title="Close" className={`${iconBtn} hover:text-red-400`}>
          <X size={11} />
        </button>
      </div>
    );
  };

  return (
    <Popover onDismiss={onDismiss} width={320}>
      <div className="relative p-1 shrink-0">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500" size={12} />
        <input
          autoFocus
          value={q}
          onChange={(e) => { setQ(e.target.value); setHi(0); }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              if (order.length) setHi((at + (e.key === "ArrowDown" ? 1 : order.length - 1)) % order.length);
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && order[at]) {
              e.preventDefault();
              // Walled ones go to their Wall, as clicking the row does.
              go(order[at]);
            }
          }}
          placeholder={`Find among ${plural(sessions.length, "session")}…`}
          className="w-full h-8 pl-7 pr-2 bg-zinc-900/60 border border-white/10 rounded-md text-[11.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
        />
      </div>
      <div className="overflow-y-auto custom-scrollbar min-h-0">
        {tabs.length > 0 && <Heading>Tabs</Heading>}
        {tabs.map((s) => row(s, false))}
        {inWalls.map(({ wall, list }) => (
          <div key={wall.id}>
            <Heading>In {wall.name}</Heading>
            {list.map((s) => row(s, true))}
          </div>
        ))}
        {shown.length === 0 && <div className="px-3 py-4 text-center text-[11px] text-zinc-600 italic">No session matches.</div>}
      </div>
      <div className="shrink-0 border-t border-white/5 mt-1 pt-1 flex items-center gap-1">
        <button
          disabled={dead.length === 0}
          onClick={() => onCloseSessions(dead)}
          className={`${menuItem} justify-center text-zinc-300 hover:bg-white/[0.06] disabled:opacity-40 disabled:hover:bg-transparent`}
        >
          Close disconnected ({dead.length})
        </button>
        <button
          disabled={sessions.length === 0}
          onClick={() => onCloseSessions(sessions.map((s) => s.id))}
          className={`${menuItem} justify-center text-rose-300 hover:bg-rose-500/10 disabled:opacity-40`}
        >
          Close all ({sessions.length})
        </button>
      </div>
    </Popover>
  );
};

// Servers to tick, by name or folder: the list a saved wall is built from,
// and "Connect more" in a Wall's Add picker.
const useServerRows = (servers: ServerRow[], folders: FolderRow[], query: string) => {
  const index = useMemo(() => indexFolders(folders), [folders]);
  const rows = useMemo(() => servers.map((s) => ({
    server: s,
    path: folderPath(index, s.folder_id ?? null).map((f) => f.name).join(" › "),
  })), [servers, index]);
  const q = query.trim().toLowerCase();
  return q
    ? rows.filter((r) => r.server.name.toLowerCase().includes(q) || r.path.toLowerCase().includes(q) || (r.server.host ?? "").toLowerCase().includes(q))
    : rows;
};

export const WallEditorModal = ({ initial, servers, folders, onSave, onClose }: {
  initial: SavedWall;
  servers: ServerRow[];
  folders: FolderRow[];
  onSave: (w: SavedWall, open: boolean) => void;
  onClose: () => void;
}) => {
  const [name, setName] = useState(initial.name);
  const [picked, setPicked] = useState<number[]>(initial.server_ids);
  const [q, setQ] = useState("");
  const rows = useServerRows(servers, folders, q);
  const pickedSet = new Set(picked);
  const allShownPicked = rows.length > 0 && rows.every((r) => pickedSet.has(r.server.id));
  const toggle = (id: number) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const ready = name.trim().length > 0 && picked.length > 0;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const save = (open: boolean) => onSave({ ...initial, name: name.trim(), server_ids: picked }, open);

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label={initial.id ? "Edit wall" : "New wall"}
        className="w-full max-w-lg max-h-[85vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <LayoutGrid size={13} className="text-indigo-300" /> {initial.id ? "Edit wall" : "New wall"}
          </span>
          <button onClick={onClose} className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>
        <div className="shrink-0 p-3 flex flex-col gap-2 border-b border-white/5">
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Name, e.g. Web servers"
            className="w-full h-9 px-3 bg-zinc-900/60 border border-white/10 rounded-lg text-[12.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
          />
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" size={13} />
              <input
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder="Find servers by name, host or folder…"
                className="w-full h-8 pl-8 pr-3 bg-zinc-900/60 border border-white/10 rounded-lg text-[12px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
              />
            </div>
            <button
              disabled={rows.length === 0}
              onClick={() => {
                const ids = rows.map((r) => r.server.id);
                setPicked((p) => (allShownPicked ? p.filter((x) => !ids.includes(x)) : [...p, ...ids.filter((x) => !p.includes(x))]));
              }}
              className="h-8 px-2.5 rounded-lg text-[10px] font-bold uppercase tracking-wider text-zinc-300 bg-white/[0.04] border border-white/10 hover:bg-white/[0.08] disabled:opacity-40"
            >
              {allShownPicked ? "Untick shown" : "Tick shown"}
            </button>
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar p-2">
          {rows.map(({ server, path }) => (
            <label key={server.id} className={`flex items-center gap-2.5 px-2.5 py-1.5 rounded-md cursor-pointer ${pickedSet.has(server.id) ? "bg-primary/10" : "hover:bg-white/[0.04]"}`}>
              <input type="checkbox" className="accent-primary shrink-0" checked={pickedSet.has(server.id)} onChange={() => toggle(server.id)} />
              <span className="text-[12px] text-zinc-200 truncate">{server.name}</span>
              {path && <span className="text-[10.5px] text-zinc-500 truncate ml-auto">{path}</span>}
            </label>
          ))}
          {rows.length === 0 && <div className="px-3 py-4 text-center text-[11.5px] text-zinc-600 italic">No server matches.</div>}
        </div>
        <div className="shrink-0 px-4 py-3 border-t border-white/5 flex items-center gap-2">
          <span className="text-[11px] text-zinc-500 flex-1">{plural(picked.length, "server")}</span>
          <button onClick={onClose} className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider text-zinc-300 hover:bg-white/5">Cancel</button>
          <button
            disabled={!ready}
            onClick={() => save(false)}
            className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-white/[0.06] border border-white/10 text-zinc-200 hover:bg-white/[0.1] disabled:opacity-40"
          >
            Save
          </button>
          <button
            disabled={!ready}
            onClick={() => save(true)}
            className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25 disabled:opacity-40"
          >
            Save &amp; open
          </button>
        </div>
      </div>
    </div>
  );
};

// A Wall's "Add": terminals of the open sessions, and servers that aren't
// open yet (they connect inside the Wall).
export const WallPicker = ({ wall, sessions, terminals, statuses, servers, folders, onToggle, onConnect, onClose }: {
  wall: Wall;
  sessions: SessionRow[];
  terminals: Record<string, { id: string; title: string }[]>;
  statuses: Record<string, string>;
  servers: ServerRow[];
  folders: FolderRow[];
  onToggle: (m: WallMember) => void;
  onConnect: (server: ServerRow) => void;
  onClose: () => void;
}) => {
  const [q, setQ] = useState("");
  const query = q.trim().toLowerCase();
  const openServerIds = new Set(sessions.map((s) => s.serverId));
  const notOpen = useServerRows(servers.filter((s) => !openServerIds.has(s.id)), folders, q);
  const shownSessions = query ? sessions.filter((s) => s.serverName.toLowerCase().includes(query)) : sessions;
  const has = (tid: string) => wall.members.some((m) => m.terminalId === tid);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div className="fixed inset-0 z-[65]" onClick={onClose} />
      <div className="absolute top-12 right-3 z-[66] w-80 max-w-[calc(100%-24px)] max-h-[70vh] bg-[#15151a] border border-white/10 rounded-xl shadow-2xl p-2 flex flex-col">
        <div className="px-1 pb-2 flex items-center gap-2 shrink-0">
          <div className="relative flex-1">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" size={12} />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="Find a session or server…"
              className="w-full h-8 pl-7 pr-2 bg-zinc-900/60 border border-white/10 rounded-md text-[11.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
            />
          </div>
          <button onClick={onClose} className="h-6 w-6 rounded flex items-center justify-center text-zinc-500 hover:text-white hover:bg-white/[0.05]">
            <X size={12} />
          </button>
        </div>
        <div className="overflow-y-auto custom-scrollbar space-y-0.5 pr-1 min-h-0">
          {shownSessions.length > 0 && <Heading>Open sessions</Heading>}
          {shownSessions.map((s) => {
            const terms = terminals[s.id] ?? [{ id: `${s.id}-term-0`, title: "1" }];
            const dot = sessionDot(statuses[s.id] ?? "connecting", false);
            return terms.map((t) => (
              <label key={t.id} className={`flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer ${has(t.id) ? "bg-primary/10" : "hover:bg-white/[0.04]"}`}>
                <input
                  type="checkbox"
                  className="accent-primary shrink-0"
                  checked={has(t.id)}
                  onChange={() => onToggle({ sessionId: s.id, terminalId: t.id })}
                />
                <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${dot.tone}`} />
                <span className="text-[11px] text-zinc-200 truncate flex-1">{s.serverName}</span>
                {terms.length > 1 && (
                  <span className="text-[10px] text-zinc-500 flex items-center gap-1 shrink-0">
                    <TerminalSquare size={10} /> {t.title}
                  </span>
                )}
              </label>
            ));
          })}
          {notOpen.length > 0 && <Heading>Connect more</Heading>}
          {notOpen.slice(0, 200).map(({ server, path }) => (
            <button
              key={server.id}
              onClick={() => onConnect(server)}
              title="Connect and add to this wall"
              className={`${menuItem} text-zinc-300 hover:bg-white/[0.06]`}
            >
              <Plus size={11} className="shrink-0 text-zinc-500" />
              <span className="truncate">{server.name}</span>
              {path && <span className="text-[10px] text-zinc-500 truncate ml-auto">{path}</span>}
            </button>
          ))}
          {notOpen.length > 200 && (
            <div className="px-2.5 py-1.5 text-[10.5px] text-zinc-500">{notOpen.length - 200} more — type to narrow it down.</div>
          )}
        </div>
      </div>
    </>
  );
};

export default WallsMenu;

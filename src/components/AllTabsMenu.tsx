import { useEffect, useRef, useState } from "react";
import { Search, X } from "lucide-react";
import { sessionDot } from "../util/sessionStatus";

// The title bar's "All tabs ▾": every open session in one searchable list
// (with a hundred open, the tab strip alone isn't a way to find one).

/// `label`: the tab's name, when it was renamed (else the server's name).
interface SessionRow { id: string; serverId: number; serverName: string; label?: string; }
/// What a session is called in its tab.
const labelOf = (s: SessionRow) => s.label ?? s.serverName;
/// A search finds a session by its tab's name or its server's.
const sessionMatches = (s: SessionRow, query: string) =>
  labelOf(s).toLowerCase().includes(query) || s.serverName.toLowerCase().includes(query);

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

export const AllTabsMenu = ({
  sessions, statuses, waiting, activeView, onShowSession, onCloseSessions, onDismiss,
}: {
  sessions: SessionRow[];
  statuses: Record<string, string>;
  waiting: Set<string>;
  activeView: string;
  /// Show it in its tab.
  onShowSession: (sessionId: string) => void;
  onCloseSessions: (ids: string[]) => void;
  onDismiss: () => void;
}) => {
  const [q, setQ] = useState("");
  const query = q.trim().toLowerCase();
  const shown = query ? sessions.filter((s) => sessionMatches(s, query)) : sessions;
  const dead = sessions.filter((s) => ["failed", "disconnected"].includes(statuses[s.id] ?? "connecting")).map((s) => s.id);
  // Enter goes to the highlighted match, the first until ↑/↓ move it.
  const [hi, setHi] = useState(0);
  const at = Math.min(hi, shown.length - 1);
  // The highlighted row is kept in view when ↑/↓ (or the search) move the
  // highlight, not on every render.
  const hiRow = useRef<HTMLButtonElement | null>(null);
  const hiId = shown[at]?.id;
  useEffect(() => { hiRow.current?.scrollIntoView({ block: "nearest" }); }, [hiId]);

  const row = (s: SessionRow) => {
    const dot = sessionDot(statuses[s.id] ?? "connecting", waiting.has(s.id));
    return (
      <div key={s.id} className="flex items-center gap-0.5">
        <button
          ref={hiId === s.id ? hiRow : undefined}
          onClick={() => onShowSession(s.id)}
          title="Show its tab"
          className={`${menuItem} flex-1 min-w-0 ${activeView === s.id ? "bg-primary/15 text-primary" : "text-zinc-200 hover:bg-white/[0.06]"} ${hiId === s.id ? "ring-1 ring-inset ring-primary/40" : ""}`}
        >
          <span title={dot.title} className={`w-1.5 h-1.5 rounded-full shrink-0 ${dot.tone}`} />
          <span className="truncate flex-1" title={labelOf(s) !== s.serverName ? s.serverName : undefined}>{labelOf(s)}</span>
        </button>
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
              if (shown.length) setHi((at + (e.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length);
            } else if (e.key === "Enter" && !e.nativeEvent.isComposing && shown[at]) {
              e.preventDefault();
              onShowSession(shown[at].id);
            }
          }}
          placeholder={`Find among ${plural(sessions.length, "session")}…`}
          className="w-full h-8 pl-7 pr-2 bg-zinc-900/60 border border-white/10 rounded-md text-[11.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
        />
      </div>
      <div className="overflow-y-auto custom-scrollbar min-h-0">
        {shown.map((s) => row(s))}
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

export default AllTabsMenu;

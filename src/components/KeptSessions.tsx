import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { History, Loader2, X } from "lucide-react";
import { useFocusTrap } from "../ui/confirm";
import { LearnMore } from "./settings/parts";
import {
  KEEP_CHANGED, KEPT_EVENT, SHOW_SESSIONS, firstKept, keepsSessions, keptSessionDetail, keptSessionLabel,
  readKeepDefault, readKeepOverride, sortKeptSessions, writeKeepDefault, writeKeepOverride,
  type KeepOverride, type KeptNews, type KeptSession, type KeptSessionList, type ShowSessions,
} from "../util/keepSession";

// Sessions that survive a reconnect (#126): the page's part. Nothing here
// ever asks: a closed tab detaches, a reconnect attaches again, and a
// session ends when the user types `exit` in it or ends it in the list.
//
// - KeptSessions: mounted with each server tab (SessionView). Tells the
//   backend whether the tab's terminals keep their sessions and which are
//   open, says once ever what a kept session means, and shows the list.
// - KeptMark: the mark on a kept terminal's tab.
// - skipIfKept: on-connect commands only for a new session.
// - KeepSessionSetting (Settings) and KeepSessionOverride (the server form).
// - showKeptSessions: "Sessions…" in the session tree's server menu.

type Tab = { id: string; serverName: string; serverUuid?: string | null; protocol?: string };

type KeptState = { kept: boolean; running: boolean };

/** `run` (a terminal's on-connect commands), unless the terminal picked up a
 *  session that was still running: they're only for a new one. */
export function skipIfKept(terminalId: string, run: () => void): () => void {
  return () => {
    invoke<KeptState>("keep_session_state", { terminalId }).then((s) => { if (!s.running) run(); }, run);
  };
}

/** Shows the sessions list of the server with sync id `uuid`, over one of its
 *  open tabs; false when none is open. */
export function showKeptSessions(uuid: string): boolean {
  const detail: ShowSessions = { uuid, claimed: false };
  window.dispatchEvent(new CustomEvent<ShowSessions>(SHOW_SESSIONS, { detail }));
  return detail.claimed;
}

/** Whether the backend says `terminalId` keeps its session, kept up to date:
 *  its news, and its shell ending (`exit` ends the session too). */
function useKept(terminalId: string): boolean {
  const [kept, setKept] = useState(false);
  useEffect(() => {
    let live = true;
    invoke<KeptState>("keep_session_state", { terminalId }).then((s) => { if (live) setKept(s.kept); }, () => {});
    const offNews = listen<KeptNews>(KEPT_EVENT, (e) => {
      if (e.payload?.terminalId === terminalId) setKept(!!e.payload.kept);
    });
    const offEnd = listen<{ shell_ended?: boolean }>(`terminal-closed-${terminalId}`, (e) => {
      if (e.payload?.shell_ended) setKept(false);
    });
    return () => {
      live = false;
      offNews.then((f) => f());
      offEnd.then((f) => f());
    };
  }, [terminalId]);
  return kept;
}

/** A small mark on a kept terminal's tab. */
export function KeptMark({ terminalId }: { terminalId: string }) {
  const kept = useKept(terminalId);
  if (!kept) return null;
  return (
    <span title="Keeps running on the server" aria-label="Keeps running on the server" className="shrink-0 text-primary/70">
      <History size={10} />
    </span>
  );
}

/** Mounted with a server tab. `openTerminal` adds a terminal with that id to
 *  the tab. */
export function KeptSessions({ session, terminals, openTerminal }: {
  session: Tab;
  terminals: { id: string }[];
  openTerminal: (id: string) => void;
}) {
  const uuid = session.serverUuid || null;
  const ssh = (session.protocol ?? "ssh") === "ssh";
  const [on, setOn] = useState(() => keepsSessions(uuid));
  const [shown, setShown] = useState(false);
  const [toast, setToast] = useState(false);
  const terminalsRef = useRef(terminals);
  terminalsRef.current = terminals;

  useEffect(() => {
    const changed = () => setOn(keepsSessions(uuid));
    const show = (e: Event) => {
      const d = (e as CustomEvent<ShowSessions>).detail;
      if (!ssh || !uuid || !d || d.claimed || d.uuid !== uuid) return;
      d.claimed = true;
      setShown(true);
    };
    window.addEventListener(KEEP_CHANGED, changed);
    window.addEventListener(SHOW_SESSIONS, show);
    return () => {
      window.removeEventListener(KEEP_CHANGED, changed);
      window.removeEventListener(SHOW_SESSIONS, show);
    };
  }, [uuid, ssh]);

  // The first time ever a terminal of this device is kept, a word on what
  // that means, which goes by itself.
  useEffect(() => {
    const off = listen<KeptNews>(KEPT_EVENT, (e) => {
      const id = e.payload?.terminalId;
      if (!e.payload?.kept || !terminalsRef.current.some((t) => t.id === id)) return;
      if (firstKept()) setToast(true);
    });
    return () => { off.then((f) => f()); };
  }, []);
  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(false), 8000);
    return () => window.clearTimeout(t);
  }, [toast]);

  useEffect(() => {
    if (!ssh) return;
    invoke("keep_session_tab", { sessionId: session.id, serverUuid: uuid, on, terminals: terminals.map((t) => t.id) })
      .catch(() => {});
  }, [session.id, uuid, on, terminals, ssh]);
  // The tab closed: nothing of it is kept here (its sessions run on).
  useEffect(() => () => {
    invoke("keep_session_tab", { sessionId: session.id, serverUuid: null, on: false, terminals: [] }).catch(() => {});
  }, [session.id]);

  return (
    <>
      {shown && <SessionsList session={session} on={on} onClose={() => setShown(false)} openTerminal={openTerminal} />}
      {toast && createPortal(
        <div
          role="status"
          onClick={() => setToast(false)}
          className="fixed bottom-6 right-6 z-[9999] max-w-[320px] px-3 py-2 rounded-lg bg-[#15151a] border border-primary/30 shadow-2xl text-[12px] text-zinc-200 flex items-start gap-2 cursor-default"
        >
          <History size={13} className="text-primary shrink-0 mt-0.5" />
          <span>This session keeps running on the server. Type exit to end it.</span>
        </div>,
        document.body,
      )}
    </>
  );
}

/** A server's kept sessions, fetched when shown (never polled), each with
 *  Open and End. */
function SessionsList({ session, on, onClose, openTerminal }: {
  session: Tab;
  on: boolean;
  onClose: () => void;
  openTerminal: (id: string) => void;
}) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  useFocusTrap(1, panelRef);
  const [list, setList] = useState<KeptSessionList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // End takes a second click on the same row: no dialog, but no slip either.
  const [ending, setEnding] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    invoke<KeptSessionList>("keep_session_list", { sessionId: session.id, serverUuid: session.serverUuid ?? null })
      .then(setList, (e) => setError(String(e)));
  }, [session.id, session.serverUuid]);
  useEffect(() => { load(); }, [load]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); onClose(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const open = (s: KeptSession) => {
    const id = `${session.id}-term-${Date.now()}`;
    invoke("keep_session_open", { sessionId: session.id, terminalId: id, name: s.name })
      .then(() => { openTerminal(id); onClose(); }, (e) => setError(String(e)));
  };
  const end = (s: KeptSession) => {
    const tool = list?.tool;
    if (!tool) return;
    if (ending !== s.name) { setEnding(s.name); return; }
    setEnding(null);
    setBusy(s.name);
    invoke("keep_session_end", { sessionId: session.id, tool, name: s.name, pid: s.pid })
      .then(load, (e) => setError(String(e)))
      .finally(() => setBusy(null));
  };

  const sessions = list ? sortKeptSessions(list.sessions) : [];
  const button = "px-2.5 h-7 rounded text-[10.5px] font-bold uppercase tracking-wider border disabled:opacity-50";
  return createPortal(
    <div className="fixed inset-0 z-[9998] bg-black/60 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="kept-sessions-title"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-[480px] max-h-[80vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl font-mono text-[12px]"
      >
        <div className="flex items-start justify-between gap-3 p-4 pb-2">
          <div className="flex items-center gap-2 min-w-0">
            <History size={14} className="text-primary shrink-0" />
            <span id="kept-sessions-title" className="text-[11px] font-black uppercase tracking-widest text-zinc-200 truncate">
              Sessions on {session.serverName}
            </span>
          </div>
          <button aria-label="Close" onClick={onClose} className="text-zinc-500 hover:text-white shrink-0">
            <X size={12} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto custom-scrollbar px-4 pb-2">
          {error ? (
            <p className="text-rose-300 break-words">{error}</p>
          ) : !list ? (
            <p className="flex items-center gap-2 text-zinc-500"><Loader2 size={12} className="animate-spin" /> Asking the server…</p>
          ) : !list.tool ? (
            <p className="text-zinc-400 leading-relaxed">
              This server has neither tmux nor screen, so its terminals aren't kept. Submarine never installs anything on a server.
            </p>
          ) : sessions.length === 0 ? (
            <p className="text-zinc-500">No kept sessions are running on this server.</p>
          ) : (
            <ul className="space-y-1.5">
              {sessions.map((s) => (
                <li key={`${s.pid ?? ""}.${s.name}`} className="flex items-center gap-3 rounded-lg border border-white/10 bg-white/[0.02] px-3 py-2">
                  <div className="flex-1 min-w-0">
                    <div className="text-zinc-200 truncate">{keptSessionLabel(s)}</div>
                    <div className="text-[11px] text-zinc-500 truncate">{keptSessionDetail(s)}</div>
                  </div>
                  <button
                    disabled={busy !== null}
                    onClick={() => open(s)}
                    title="Open it in a new terminal of this tab"
                    className={`${button} bg-primary/15 border-primary/40 text-primary hover:bg-primary/25`}
                  >
                    Open
                  </button>
                  <button
                    disabled={busy !== null}
                    onClick={() => end(s)}
                    onBlur={() => setEnding((n) => (n === s.name ? null : n))}
                    title="End it for good, with whatever runs in it"
                    className={`${button} bg-rose-500/10 border-rose-500/40 text-rose-300 hover:bg-rose-500/20`}
                  >
                    {busy === s.name ? <Loader2 size={11} className="animate-spin" /> : ending === s.name ? "End it?" : "End"}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <p className="px-4 py-3 text-[11px] text-zinc-500 leading-relaxed border-t border-white/5">
          {on
            ? "Closing a tab leaves its session running. Type exit in it, or End it here, when you're done."
            : "This server's new terminals aren't kept: Settings › Servers & sessions, or the server's own settings, turn it on."}
        </p>
      </div>
    </div>,
    document.body,
  );
}

/** Settings › Servers & sessions: the choice for every server on this computer. */
export function KeepSessionSetting() {
  const [on, setOn] = useState(readKeepDefault);
  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 shadow-xl">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-zinc-100">Keep sessions between connections</div>
          <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
            A dropped connection or a closed tab leaves the terminal running, and connecting again takes you back.
          </p>
          <LearnMore>
            New terminals run in tmux on the server, or screen where there's no tmux, so sleep leaves them running
            too. Type exit to end one. Nothing is installed on a server. A server's own settings can say
            otherwise. Kept on this computer only.
          </LearnMore>
        </div>
        <button
          role="switch"
          aria-checked={on}
          aria-label="Keep sessions between connections"
          onClick={() => { writeKeepDefault(!on); setOn(!on); }}
          className={`relative shrink-0 w-11 h-6 rounded-full transition-colors ${on ? "bg-primary" : "bg-zinc-700"}`}
        >
          <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${on ? "left-[22px]" : "left-0.5"}`} />
        </button>
      </div>
    </div>
  );
}

/** The server form: this server's own choice, on this computer. Saved as
 *  it's picked, like the other choices kept on this device. */
export function KeepSessionOverride({ uuid }: { uuid?: string | null }) {
  const [choice, setChoice] = useState<KeepOverride>(() => readKeepOverride(uuid));
  if (!uuid) return null;
  return (
    <div className="space-y-1.5 pt-1">
      <label htmlFor="keep-session-choice" className="text-[11px] font-bold text-zinc-400 ml-1 flex items-center gap-2">
        <span>Keep sessions between connections</span>
        <span className="text-[10px] text-zinc-600 font-normal normal-case tracking-normal">On this computer only.</span>
      </label>
      <select
        id="keep-session-choice"
        value={choice}
        onChange={(e) => {
          const next = e.target.value as KeepOverride;
          writeKeepOverride(uuid, next);
          setChoice(next);
        }}
        className="w-full bg-[#1a1a1e] rounded-lg px-3 h-9 text-[12px] text-white border border-white/10 outline-none focus:border-primary/50"
      >
        <option value="default">Use the default ({readKeepDefault() ? "on" : "off"})</option>
        <option value="always">Always</option>
        <option value="never">Never</option>
      </select>
    </div>
  );
}

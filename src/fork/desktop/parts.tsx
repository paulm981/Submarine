import { useEffect, useState } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, Home, List } from "lucide-react";
import AiPanel from "../../components/AiPanel";
import AiSidePane, { AiPaneButton, type AiPaneView } from "../../components/AiSidePane";
import AllTabsMenu from "../../components/AllTabsMenu";
import HomePane from "../../components/HomePane";
import { SessionTree } from "../../components/SessionTree";
import { useAiPane } from "../../ui/aiPane";
import { titleTabClass } from "../../ui/tabStyle";
import { reorderById } from "../../ui/tabDrag";
import { aiMarker, cardsOf, loosestShare, openAiTrust, sharedWith, trustTargetFor, useAi } from "../../util/ai";
import { type CloseWhich, HAS_HOME, closeCounts } from "./rules";
import type { SessionTreeState } from "./hooks";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of the main window, each rendered by DesktopApp in one
// line (#163): DesktopApp.tsx is the original project's file. Each renders
// what DesktopApp did in its place, or nothing where it didn't.
// ─────────────────────────────────────────────────────────────────────────────

/** The version in the title bar, from the build itself (about::app_info)
 *  rather than hardcoded so it can never drift from the real build: a
 *  release's whole version (v0.4.0-fork.2), and the commit in the tooltip
 *  when built from source. Shown faded next to the brand name; renders
 *  nothing if the call ever fails. */
export const AppVersion = () => {
  const [appVersion, setAppVersion] = useState<{ version: string; commit: string | null } | null>(null);
  useEffect(() => {
    invoke<{ version: string; commit: string | null }>("app_info").then(setAppVersion).catch(() => {});
  }, []);
  if (!appVersion) return null;
  return (
    <span
      data-tauri-drag-region
      title={appVersion.commit ? `Built from commit ${appVersion.commit}` : undefined}
      className="text-[11px] text-zinc-500/60 tracking-tight tabular-nums"
    >
      v{appVersion.version}
    </span>
  );
};

/** Home's tab, first in the title bar, like MobaXterm's home tab: always
 *  there to go back to, and not a session (no status, no close, never in a
 *  broadcast or a split). */
export const HomeTab = ({ show, active, compact, onClick }: { show: boolean; active: boolean; compact: boolean; onClick: () => void }) => {
  if (!HAS_HOME || !show) return null;
  return (
    <div
      onClick={onClick}
      title="Home — terminals on this computer"
      aria-label="Home"
      className={`no-drag flex items-center justify-center h-7 w-9 cursor-pointer transition-all shrink-0 ${
        titleTabClass({ compact, active })
      }`}
    >
      <Home size={14} className="shrink-0" />
    </div>
  );
};

/** Home: terminals on this computer. Mounted the first time it's shown, or
 *  when an AI tool started from a server tab needs it (#100), and only
 *  hidden after that (like the sessions canvas), so its shells keep running
 *  while the user is in a session. `shells` counts those still running,
 *  which closing the window or locking the profile ends too. */
export const HomeView = ({ visible, shells }: { visible: boolean; shells: MutableRefObject<number> }) => {
  const [shown, setShown] = useState(false);
  useEffect(() => { if (visible) setShown(true); }, [visible]);
  const { homeNeeded } = useAiPane();
  if (!HAS_HOME || !(shown || homeNeeded)) return null;
  return (
    <div className={`absolute inset-0 flex flex-col overflow-hidden bg-background ${visible ? "" : "opacity-0 pointer-events-none"}`}>
      <HomePane visible={visible} onLiveShells={(n) => { shells.current = n; }} />
    </div>
  );
};

/** On a server tab: shared with an AI tab, lit while one is doing something
 *  there. Amber, with a click to change it, where the AI may run commands
 *  there without asking (#115). */
export const TabAiMarker = ({ sessionId }: { sessionId: string }) => {
  const ai = useAi();
  const with_ = sharedWith(ai, sessionId);
  if (with_.length === 0) return null;
  const busy = ai.running.some(r => r.session_id === sessionId) || ai.pending.some(p => p.session_id === sessionId);
  const entries = with_.flatMap(t => t.servers.filter(x => x.session_id === sessionId));
  const loosest = loosestShare(entries);
  const marker = loosest ? aiMarker(loosest) : null;
  if (loosest && marker) {
    return (
      <button
        type="button"
        onClick={(e) => { e.stopPropagation(); openAiTrust(trustTargetFor(loosest)); }}
        title={`Shared with ${with_.map(t => t.label).join(", ")}. ${marker}. Click to change it.`}
        aria-label={marker}
        className={`mr-1 shrink-0 flex items-center gap-0.5 rounded px-0.5 text-amber-300 hover:bg-amber-400/15 ${busy ? "animate-pulse" : ""}`}
      >
        <Bot size={10} />
        {/* The marker's words, the Bot icon standing for "AI:". */}
        <span className="text-[9.5px] font-semibold whitespace-nowrap">{marker.replace(/^AI: /, "")}</span>
      </button>
    );
  }
  return (
    <span title={`Shared with ${with_.map(t => t.label).join(", ")}`} aria-label="Shared with AI" className={`mr-1 shrink-0 flex ${busy ? "text-primary animate-pulse" : "text-zinc-400"}`}>
      <Bot size={10} />
    </span>
  );
};

/** All tabs ▾ in the title bar: every session, searchable (Ctrl+Shift+K),
 *  anchored at the right edge however many tabs are open. */
export const AllTabsButton = <S extends { id: string; serverId: number; serverName: string }>({
  show, open, setOpen, sessions, label, statuses, waiting, activeView, onShow, onClose,
}: {
  show: boolean; open: boolean; setOpen: Dispatch<SetStateAction<boolean>>;
  sessions: S[]; label: (s: S) => string; statuses: Record<string, string>; waiting: Set<string>; activeView: string;
  /** Show this session in its tab (out of any split). */
  onShow: (sid: string) => void;
  onClose: (ids: string[]) => Promise<boolean>;
}) => {
  if (!show || sessions.length === 0) return null;
  return (
    <div className="relative no-drag shrink-0 mx-0.5">
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        title={`All sessions (${sessions.length}) — Ctrl+Shift+K to jump by name`}
        aria-label="All sessions"
        className={`relative flex items-center justify-center w-7 h-7 rounded-full transition-all ${
          open
            ? "bg-primary/15 border border-primary/40 text-primary"
            : "bg-white/[0.06] border border-white/10 text-zinc-400 hover:bg-white/[0.1] hover:text-white"
        }`}
      >
        <List size={12} />
        <span
          className="absolute -top-1 -right-1 min-w-[14px] h-[14px] px-1 rounded-full bg-zinc-300 text-[9px] font-bold text-black flex items-center justify-center leading-none"
          aria-hidden="true"
        >
          {sessions.length}
        </span>
      </button>
      {open && (
        <AllTabsMenu
          sessions={sessions.map(s => ({ ...s, label: label(s) }))}
          statuses={statuses}
          waiting={waiting}
          activeView={activeView}
          onShowSession={(sid) => { setOpen(false); onShow(sid); }}
          onCloseSessions={(ids) => { void onClose(ids); }}
          onDismiss={() => setOpen(false)}
        />
      )}
    </div>
  );
};

/** The title bar's AI buttons: the side pane's (desktop) and the AI panel's,
 *  which counts the requests waiting for you. */
export const AiTitleButtons = ({ show, isMobile, v, open, onToggle }: {
  show: boolean; isMobile: boolean; v: AiPaneView; open: boolean; onToggle: () => void;
}) => {
  const ai = useAi();
  if (!show || !ai.enabled) return null;
  const waiting = cardsOf(ai.pending).length + (ai.server_asks ?? []).length;
  return (
    <div className="relative no-drag shrink-0 mx-1 flex items-center gap-1">
      {HAS_HOME && !isMobile && <AiPaneButton v={v} />}
      <button
        type="button"
        onClick={onToggle}
        title={waiting ? `AI: ${waiting} waiting for you` : "AI panel"}
        aria-label="AI panel"
        aria-pressed={open}
        className={`relative flex items-center justify-center w-7 h-7 rounded-full transition-all ${
          open || waiting
            ? "bg-primary/15 border border-primary/40 text-primary"
            : "bg-white/[0.06] border border-white/10 text-zinc-400 hover:bg-white/[0.1] hover:text-white"
        }`}
      >
        <Bot size={13} />
        {waiting > 0 && (
          <span className="absolute -top-1 -right-1 min-w-[14px] h-[14px] px-1 rounded-full bg-primary text-[9px] font-bold text-black flex items-center justify-center leading-none" aria-hidden="true">
            {waiting}
          </span>
        )}
      </button>
    </div>
  );
};

const menuItem = "w-full text-left px-3 py-1.5 hover:bg-white/10 text-zinc-200 disabled:opacity-40 disabled:hover:bg-transparent";

/** In a tab's right-click menu: moving the tab without dragging it
 *  (keyboard, touchpad). */
export const TabMoveItems = <S extends { id: string }>({ sessions, targetId, setSessions, onDone }: {
  sessions: S[]; targetId: string; setSessions: (f: (prev: S[]) => S[]) => void; onDone: () => void;
}) => {
  const tabs = sessions.map(s => s.id);
  const at = tabs.indexOf(targetId);
  const moveTo = (to: number) => {
    setSessions(prev => reorderById(prev, targetId, tabs[to], to > at));
    onDone();
  };
  return (
    <>
      <button disabled={at <= 0} className={menuItem} onClick={() => moveTo(at - 1)}>Move left</button>
      <button disabled={at < 0 || at >= tabs.length - 1} className={menuItem} onClick={() => moveTo(at + 1)}>Move right</button>
    </>
  );
};

/** In a tab's right-click menu: closing the tabs other than it, after it,
 *  the disconnected ones, or every one, each with how many. */
export const TabCloseItems = ({ tabs, targetId, statuses, onPick }: {
  tabs: string[]; targetId: string; statuses: Record<string, string>; onPick: (which: CloseWhich) => void;
}) => {
  const n = closeCounts(tabs, targetId, statuses);
  const item = (label: string, which: CloseWhich) => (
    <button
      disabled={n[which] <= 0}
      className="w-full text-left px-3 py-1.5 hover:bg-rose-500/15 hover:text-rose-300 text-zinc-200 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-zinc-200"
      onClick={() => onPick(which)}
    >{label} ({n[which]})</button>
  );
  return (
    <>
      {item("Close other tabs", "others")}
      {item("Close tabs to the right", "right")}
      {item("Close disconnected tabs", "dead")}
      {item("Close all tabs", "all")}
    </>
  );
};

/** The AI side pane beside a server tab (#100), on the desktop. */
export const DesktopAiSide = ({ v, isMobile, sessions, setActiveView }: {
  v: AiPaneView; isMobile: boolean; sessions: { id: string }[]; setActiveView: (view: string) => void;
}) => {
  if (!HAS_HOME || isMobile) return null;
  return (
    <AiSidePane
      v={v}
      onHome={() => setActiveView("home")}
      onGoToServer={(sid) => { if (sessions.some(x => x.id === sid)) setActiveView(sid); }}
    />
  );
};

/** The AI panel (the title bar's AI button), while AI tools are on. */
export const DesktopAiPanel = ({ open, onClose, sessions, setActiveView }: {
  open: boolean; onClose: () => void; sessions: { id: string }[]; setActiveView: (view: string) => void;
}) => {
  const ai = useAi();
  if (!open || !ai.enabled) return null;
  return (
    <AiPanel
      snap={ai}
      onClose={onClose}
      onGoToServer={(sid) => { if (sessions.some(x => x.id === sid)) setActiveView(sid); }}
    />
  );
};

/** The session tree on the desktop, on its side of the window when it's open. */
export const DesktopSessionTree = ({ tree, side, isMobile }: { tree: SessionTreeState; side: "left" | "right"; isMobile: boolean }) =>
  !isMobile && tree.open && tree.side === side ? <SessionTree {...tree.props} /> : null;

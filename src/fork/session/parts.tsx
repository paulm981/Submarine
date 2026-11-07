import type { Dispatch, SetStateAction } from "react";
import { createPortal } from "react-dom";
import { Bot, Columns, Container, Rows, SplitSquareHorizontal, TerminalSquare } from "lucide-react";
import { KeptMark } from "../../components/KeptSessions";
import { reorderById } from "../../ui/tabDrag";
import { COMPACT_TAB_NAME, NewsDot } from "../../ui/tabStyle";
import type { JumpGone, ShareWithAi } from "./hooks";
import type { SessionTerminal } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of a server's tab, each rendered by SessionView in one
// line (#163): SessionView.tsx is the original project's file. Each renders
// what SessionView did in its place, or nothing where it didn't.
// ─────────────────────────────────────────────────────────────────────────────

/** The choices for a server whose jump host was deleted (useJumpGone), once
 *  a connect was refused for it; in a box of its own (`box`) over the
 *  terminals. */
export const JumpGoneChoices = ({ jumpGone, box }: { jumpGone: JumpGone; box?: string }) => {
  if (!jumpGone.note) return null;
  const choices = (
    <div role="alert" className="mb-3 sm:mb-4 px-3 py-2.5 rounded-xl border border-amber-500/30 bg-amber-500/10 text-amber-200 text-[12px] leading-relaxed">
      <p>{jumpGone.note}</p>
      <div className="mt-2 flex flex-wrap gap-2">
        <button
          onClick={jumpGone.connectDirectly}
          className="h-7 px-3 rounded-lg bg-amber-500/15 border border-amber-500/40 text-amber-100 hover:bg-amber-500/25 text-[11px] font-semibold"
        >
          Connect directly
        </button>
        <button
          onClick={jumpGone.pickJumpHost}
          className="h-7 px-3 rounded-lg bg-white/5 border border-white/10 text-zinc-200 hover:bg-white/10 hover:text-white text-[11px] font-semibold"
        >
          Pick a jump host
        </button>
      </div>
    </div>
  );
  return box ? <div className={box}>{choices}</div> : choices;
};

/** What a terminal tab shows: its icon, a dot when it printed something
 *  while out of sight (cleared when it comes on screen, so never on the tab
 *  in front), the kept-session mark, and its name (cut on a compact tab;
 *  the full one is the hover title). */
export const TermTabLabel = ({ t, compactTabs }: { t: SessionTerminal; compactTabs: boolean }) => (
  <>
    {t.container
      ? <Container size={compactTabs ? 12 : 14} className="text-sky-300 shrink-0" />
      : <TerminalSquare size={compactTabs ? 12 : 14} className="shrink-0" />}
    <NewsDot terminals={[t.id]} />
    <KeptMark terminalId={t.id} />
    <span className={compactTabs ? COMPACT_TAB_NAME : undefined}>{t.title}</span>
  </>
);

/** Split: another shell on this server beside the one on screen (watch a log
 *  in one, work in the other). The + menu has the rest: an open tab beside
 *  this one, flip, end the split. Not in a compact window. */
export const SplitButton = ({ show, splitOn, orientation, onClick, onMenu }: {
  show: boolean;
  splitOn: boolean;
  orientation: "h" | "v";
  onClick: () => void;
  onMenu: () => void;
}) => !show ? null : (
  <button
    onClick={onClick}
    onContextMenu={(e) => {
      e.preventDefault();
      onMenu();
    }}
    className={`h-10 w-10 sm:h-8 sm:w-8 shrink-0 rounded-lg flex items-center justify-center transition-all border ${
      splitOn
        ? 'text-primary border-primary/30 bg-primary/10 hover:bg-primary/20'
        : 'text-zinc-500 border-dashed border-white/10 hover:bg-white/10 hover:text-white'
    }`}
    title={splitOn
      ? "Add another pane: one more shell on this server · right-click for more"
      : "Split: another shell on this server, side by side · right-click for more"}
    aria-label="Split"
  >
    {orientation === "h" ? <Columns size={14} /> : <Rows size={14} />}
  </button>
);

/** The + menu: an open tab beside the one(s) on screen. */
export const SplitBesideItems = ({ terminals, activeTab, splitOn, splitTerminals, pick }: {
  terminals: SessionTerminal[];
  activeTab: string;
  splitOn: boolean;
  splitTerminals: string[];
  pick: (id: string) => void;
}) => (
  <>
    {terminals
      .filter(t => t.id !== activeTab && !(splitOn && splitTerminals.includes(t.id)))
      .map(t => (
        <button
          key={t.id}
          onClick={() => pick(t.id)}
          className="w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-300 hover:text-white text-left"
        >
          <SplitSquareHorizontal size={13} className="text-zinc-500" />
          <span className="flex-1 truncate">Show tab {t.title} beside this one</span>
        </button>
      ))}
  </>
);

/** The tool rail's Share with AI: while AI tools are on (Settings › AI
 *  tools). Lit while shared; a click then stops sharing. With more than one
 *  AI tab, a click opens ShareWithAiMenu. */
export const ShareWithAiButton = ({ share, status, sessionId }: { share: ShareWithAi; status: string; sessionId: string }) => {
  const { ai, aiSharedWith, setAiMenu, shareWithAi, stopSharingWithAi } = share;
  if (!ai.enabled) return null;
  return (
    <button
      onClick={(e) => {
        if (aiSharedWith.length > 0) return stopSharingWithAi();
        if (ai.tabs.length === 1) return shareWithAi(ai.tabs[0].id);
        const r = e.currentTarget.getBoundingClientRect();
        setAiMenu({ x: r.right, y: r.bottom + 4 });
      }}
      disabled={aiSharedWith.length === 0 && (status !== 'connected' || ai.tabs.length === 0)}
      title={aiSharedWith.length > 0
        ? `Shared with ${aiSharedWith.map(t => t.label).join(", ")} — click to stop sharing`
        : ai.tabs.length === 0
          ? "Share with AI: start an AI tool from Home's menu first"
          : status !== 'connected'
            ? "Share with AI: once this tab is connected"
            : "Share with AI: let an AI tab ask to run commands here. Each one waits for your Allow, unless you have set this server to run commands without asking"}
      aria-label="Share with AI"
      aria-pressed={aiSharedWith.length > 0}
      className={`h-10 sm:h-8 px-2.5 rounded-lg flex items-center gap-1.5 transition-all disabled:opacity-40 disabled:pointer-events-none ${
        aiSharedWith.length > 0
          ? 'bg-primary/10 text-primary border border-primary/20 shadow-inner'
          : 'text-zinc-300 bg-white/[0.04] border border-white/10 hover:bg-white/[0.08] hover:border-white/20 hover:text-white'
      }`}
    >
      <Bot size={14} className={`shrink-0 ${ai.running.some(r => r.session_id === sessionId) ? 'animate-pulse' : ''}`} />
      <span className="hidden md:inline text-[11px] font-semibold tracking-tight">
        {aiSharedWith.length > 0 ? `Shared with ${aiSharedWith.map(t => t.label).join(", ")}` : "Share with AI"}
      </span>
    </button>
  );
};

/** Which AI tab to share this tab with, when more than one is open. */
export const ShareWithAiMenu = ({ share, name }: { share: ShareWithAi; name: string }) => {
  const { ai, aiMenu, setAiMenu, shareWithAi } = share;
  return aiMenu && createPortal(
    <>
      <div className="fixed inset-0 z-[9998]" onClick={() => setAiMenu(null)} onContextMenu={(e) => { e.preventDefault(); setAiMenu(null); }} />
      <div
        role="menu"
        style={{ left: Math.max(8, aiMenu.x - 220), top: aiMenu.y }}
        className="fixed z-[9999] w-[220px] bg-[#15151a] border border-white/10 rounded-lg shadow-2xl py-1 text-[11.5px]"
      >
        <div className="px-3 pt-1 pb-0.5 text-[9.5px] font-bold uppercase tracking-wider text-zinc-500">Share {name} with</div>
        {ai.tabs.map((t) => (
          <button
            key={t.id}
            role="menuitem"
            onClick={() => shareWithAi(t.id)}
            className="w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left"
          >
            <Bot size={13} className="text-zinc-500 shrink-0" />
            <span className="flex-1 truncate">{t.label}</span>
          </button>
        ))}
      </div>
    </>,
    document.body,
  );
};

/** The + menu's "Claude Code beside this" (useShareWithAi), one item per AI
 *  tool. */
export const AiBesideItems = ({ share, status, isCompact }: { share: ShareWithAi; status: string; isCompact: boolean }) => (
  <>
    {share.besideTools.map((tool, i) => (
      <button
        key={tool.id}
        onClick={() => share.startAiBeside(tool)}
        disabled={status !== 'connected'}
        title={status !== 'connected'
          ? `${tool.label} beside this: once this tab is connected`
          : `Start ${tool.label} in Home, share this tab with it, and show it beside this tab`}
        className={`w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left disabled:opacity-40 disabled:pointer-events-none ${
          i === 0 && !isCompact ? 'mt-1 pt-1.5 border-t border-white/5' : ''
        }`}
      >
        <Bot size={13} className="text-primary/70" />
        <span className="flex-1">{tool.label} beside this</span>
      </button>
    ))}
  </>
);

/** A terminal tab's menu: moving the tab without dragging it (keyboard,
 *  touchpad). */
export const TermMoveItems = ({ terminals, termId, setTerminals, close }: {
  terminals: SessionTerminal[];
  termId: string;
  setTerminals: Dispatch<SetStateAction<SessionTerminal[]>>;
  close: () => void;
}) => {
  if (terminals.length <= 1) return null;
  const at = terminals.findIndex(t => t.id === termId);
  const moveTo = (to: number) => {
    setTerminals(prev => reorderById(prev, termId, terminals[to].id, to > at));
    close();
  };
  const item = "w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left disabled:opacity-40 disabled:hover:bg-transparent";
  return (
    <>
      <button disabled={at <= 0} onClick={() => moveTo(at - 1)} className={item}>
        <span className="flex-1">Move left</span>
      </button>
      <button disabled={at >= terminals.length - 1} onClick={() => moveTo(at + 1)} className={item}>
        <span className="flex-1">Move right</span>
      </button>
    </>
  );
};

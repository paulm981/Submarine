import { COMPACT_TAB_PAD, compactTermTabClass } from "../../ui/tabStyle";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's rules for a server's tab (SessionView), as plain values and
// functions that SessionView and the rest of src/fork/session use (#163).
//
// Some of them are the fork's look of an element the original project still
// draws: SessionView keeps the original project's attributes and spreads one
// of these after them, so the fork's win and the original project's lines
// stay as they are. Each returns what SessionView had in their place.
// ─────────────────────────────────────────────────────────────────────────────

/** One of a server tab's terminals (SessionView's `terminals`). */
export type SessionTerminal = {
  id: string;
  title: string;
  baseTitle?: string;
  container?: { name: string; useSudo: boolean };
  ownLogin?: boolean;
  /** The assistant's terminal (upstream's AI tab): no shell runs in it. */
  ai?: boolean;
};

/** What the fork reads of a server tab's session (DesktopApp's Session). */
export type SessionInfo = {
  id: string;
  serverId: number;
  serverName: string;
  runOnConnect?: string;
  runOnConnectEvery?: boolean;
  runOnConnectHeld?: boolean;
  quickAuth?: { host: string; port: number; username: string } | null;
};

/** DesktopApp's addLog. */
export type AddLog = (msg: string, type?: string) => void;

/** A line of the connect view's log (SessionView's pushLog). */
export type LogEntry = { msg: string; type: string };

// The start of a connect refused because the server's jump host was deleted
// (src-tauri/src/jump_gone.rs, CODE).
export const JUMP_GONE = "[JUMP_GONE]";

/** The terminal tab strip: no gap between compact tabs. */
export const stripLook = (compactTabs: boolean): { className?: string } => ({
  className: `flex items-center ${compactTabs ? "gap-0" : "gap-1"} overflow-x-auto no-scrollbar flex-1 mr-1 sm:mr-4 mask-fade-right`,
});

/** A terminal tab's button: its whole name as the hover title (compact tabs
 *  cut it), and the compact look (Settings). */
export const termTabLook = (
  t: SessionTerminal, compactTabs: boolean, isFocusedHalf: boolean, isSplitPartner: boolean, count: number, aiFull = false,
): { title?: string; className?: string } => ({
  title: t.container ? `${t.title} · Container: ${t.container.name}` : t.title,
  className: `h-10 sm:h-8 ${compactTabs
    ? `${COMPACT_TAB_PAD} ${count > 1 ? 'pr-6' : ''} gap-1.5 text-[10px] ${
        compactTermTabClass(isFocusedHalf, isSplitPartner ? "merged" : "primary")
      }`
    : `px-3 sm:px-4 ${count > 1 ? 'pr-8 sm:pr-8' : ''} rounded-lg gap-2 text-[11px] ${
        isFocusedHalf
          ? 'bg-primary/10 text-primary border border-primary/20 shadow-inner'
          : isSplitPartner
            ? 'bg-primary/[0.04] text-primary/70 border border-primary/10'
            : 'text-zinc-300 bg-white/[0.04] border border-white/10 hover:bg-white/[0.08] hover:border-white/20 hover:text-white'
      }`
  } flex items-center font-bold uppercase tracking-wider transition-all ${t.container ? 'ring-1 ring-sky-400/30' : ''} ${t.ai && aiFull ? 'ring-1 ring-rose-500/60' : ''}`,
});

/** A terminal tab's close button: never starts a drag (ui/tabDrag), and
 *  smaller on a compact tab. */
export const termCloseLook = (compactTabs: boolean): { "data-no-tab-drag": boolean; className?: string } => ({
  "data-no-tab-drag": true,
  className: `absolute right-0 sm:right-1 top-1/2 -translate-y-1/2 w-8 h-10 ${compactTabs ? 'sm:w-4 sm:h-4' : 'sm:w-5 sm:h-5'} flex items-center justify-center rounded opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100 hover:bg-white/5 hover:text-red-400 text-zinc-500 transition-all`,
});

/** A terminal's box in the terminal area. `inView`: a pane of the split on
 *  screen, or the tab in front. A terminal behind another tab is left out of
 *  the layout (`hidden`), so xterm stops drawing it (#127); it keeps its
 *  buffer, its shell keeps running, and it's never remounted. `group/pane`
 *  shows a pane's "take out of the split" button on hover. */
export const paneLook = (inSplit: boolean, inView: boolean, isFocused: boolean): { className?: string } => ({
  className: `group/pane absolute ${inSplit ? 'min-w-0 min-h-0 border' : 'inset-0'} ${
    inView ? 'opacity-100 z-10' : 'hidden'
  } ${inSplit ? (isFocused
    ? 'border-primary/40 shadow-[inset_0_0_0_1px_rgba(var(--primary),0.15)]'
    : 'border-white/[0.03]') : ''}`,
});

/** The split is on screen when it has 2+ panes, there's room, and the tab in
 *  front is one of them. Picking another tab shows that one alone; picking
 *  one of the split's tabs again brings the split back as it was. */
export const splitShown = (splitTerminals: string[], isCompact: boolean, activeTab: string): boolean =>
  splitTerminals.length >= 2 && !isCompact && splitTerminals.includes(activeTab);

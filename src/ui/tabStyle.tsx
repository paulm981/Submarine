import { useHasNews } from "./terminalActivity";

// ─────────────────────────────────────────────────────────────────────────────
// How tabs look, in one place: the title bar's tabs (Home, Broadcast, servers)
// and each server's terminal tabs. The title bar's are the original project's
// classic tabs (#339): the open one joins the toolbar under it, with a line
// on top; the fork draws Home and Broadcast's tabs the same way. Compact tabs
// (Settings › General) makes them narrower, with long names cut short, and
// makes a server's terminal tabs flat tabs that touch, with a thin line under
// the one in front.
// ─────────────────────────────────────────────────────────────────────────────

/// primary: a server or Home; merged: a server shown in a split beside the
/// one in front.
export type TabTone = "primary" | "merged";

// Colours for the compact look: `on` is the tab in front.
const COMPACT: Record<TabTone, { on: string; off: string }> = {
  primary: {
    on: "text-primary border-b-primary bg-primary/[0.07]",
    off: "text-zinc-400 border-b-transparent hover:bg-white/[0.06] hover:text-white",
  },
  merged: {
    on: "text-primary border-b-primary bg-primary/[0.07]",
    off: "text-primary/80 border-b-primary/40 hover:bg-white/[0.06]",
  },
};

/// Shape and colours of the fork's title-bar tabs (Home, Broadcast), as the
/// original project draws a server's tab in DesktopApp.tsx (#339). The
/// caller adds its own width or padding.
export const titleTabClass = ({ active }: { active: boolean }): string =>
  `relative flex items-center h-[33px] rounded-t-lg cursor-pointer shrink-0 transition-colors ${
    active ? "z-10 bg-[#141417] text-zinc-100 border-x border-t border-white/10" : "text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.04]"
  }`;

/// Inside a fork's title-bar tab: the open one's line on top (`color`), and
/// the thin line after it when neither it nor the next tab is open
/// (tabDivider), as the original project draws a server's tab.
export const TitleTabEdges = ({ active, divider, color }: { active: boolean; divider: boolean; color: string }) => (
  <>
    {active && <span aria-hidden className="absolute left-2 right-2 top-0 h-[2px] rounded-b-full" style={{ background: color }} />}
    {divider && <span aria-hidden className="absolute right-0 top-2 bottom-2 w-px bg-white/10" />}
  </>
);

/// Colours of a compact terminal tab (inside a server): same as the title
/// bar's, without the dividing line.
export const compactTermTabClass = (active: boolean, tone: TabTone = "primary"): string =>
  `rounded-none border-b-2 ${COMPACT[tone][active ? "on" : "off"]}`;

/// Padding of a compact tab with a name in it.
export const COMPACT_TAB_PAD = "px-2";
/// A compact tab cuts a long name; the full one is the hover title.
export const COMPACT_TAB_NAME = "max-w-[9rem] truncate";

/// The "new output" dot: something printed in one of these terminals (or a
/// terminal of these sessions) while you weren't looking. It clears when you
/// look at that terminal. It listens for itself, so a dot changing redraws
/// only the dot, not the tab strip around it.
export const NewsDot = ({ terminals, sessions, className = "" }: {
  terminals?: readonly string[];
  sessions?: readonly string[];
  className?: string;
}) => {
  const news = useHasNews({ terminals, sessions });
  if (!news) return null;
  return (
    <span
      title="New output"
      aria-label="New output"
      className={`w-1.5 h-1.5 rounded-full shrink-0 bg-primary ring-2 ring-primary/30 ${className}`}
    />
  );
};

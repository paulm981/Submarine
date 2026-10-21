import { useHasNews } from "./terminalActivity";

// ─────────────────────────────────────────────────────────────────────────────
// How tabs look, in one place: the title bar's tabs (Home, Walls, servers)
// and each server's terminal tabs. Two looks, picked in Settings ›
// Appearance: rounded pills, or compact — flat tabs that touch, with a thin
// line under the one in front.
// ─────────────────────────────────────────────────────────────────────────────

/// primary: a server or Home; merged: a server shown in a split beside the
/// one in front; broadcast: a Wall that is broadcasting.
export type TabTone = "primary" | "merged" | "broadcast";

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
  broadcast: {
    on: "text-orange-200 border-b-orange-400 bg-orange-500/[0.1]",
    off: "text-orange-300/90 border-b-orange-400/40 hover:bg-orange-500/10",
  },
};

// The same for the pills.
const PILL: Record<TabTone, { on: string; off: string }> = {
  primary: {
    on: "bg-primary/15 text-primary border-primary/40 shadow-inner shadow-primary/10",
    off: "bg-white/[0.06] text-zinc-300 border-white/10 hover:bg-white/[0.1] hover:border-white/20 hover:text-white",
  },
  merged: {
    on: "bg-primary/15 text-primary border-primary/40 shadow-inner shadow-primary/10",
    off: "bg-primary/[0.05] text-primary/80 border-primary/25 hover:bg-primary/10",
  },
  broadcast: {
    on: "bg-orange-500/20 text-orange-200 border-orange-400/50 shadow-inner",
    off: "bg-orange-500/[0.08] text-orange-300/90 border-orange-400/30 hover:bg-orange-500/15",
  },
};

/// Shape and colours of a title-bar tab (Home, a Wall, a server). The caller
/// adds its own width or padding (`COMPACT_TAB_PAD` in the compact look).
export const titleTabClass = ({ compact, active, tone = "primary" }: { compact: boolean; active: boolean; tone?: TabTone }): string =>
  compact
    ? `rounded-none border-b-2 border-r border-r-white/5 ${COMPACT[tone][active ? "on" : "off"]}`
    : `rounded-full mr-1 border ${PILL[tone][active ? "on" : "off"]}`;

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

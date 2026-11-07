// The tabs across the top of Settings (#168), and what shows under the one
// that's open: the original project's cards by their heading, then the
// fork's own (ForkSettings).
import { Children, isValidElement, useCallback, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Bot, Cloud, LifeBuoy, Palette, Server, TerminalSquare, type LucideIcon } from "lucide-react";
import { IS_MOBILE } from "../../util/platform";
import {
  SETTINGS_TABS, availableTabs, headingOf, readTab, tabForHeading, writeTab, type SettingsTab,
} from "./categories";
import { ForkSettings } from "./ForkSettings";

const ICONS: Record<SettingsTab, LucideIcon> = {
  general: Palette,
  terminal: TerminalSquare,
  servers: Server,
  ai: Bot,
  sync: Cloud,
  help: LifeBuoy,
};

const PANEL_ID = "settings-panel";
const tabId = (tab: SettingsTab) => `settings-tab-${tab}`;

/** The open tab, remembered on this device. */
export function useSettingsTab(): [SettingsTab, (tab: SettingsTab) => void] {
  const [tab, setTab] = useState<SettingsTab>(() => readTab(availableTabs(IS_MOBILE)));
  const choose = useCallback((next: SettingsTab) => {
    setTab(next);
    writeTab(next);
  }, []);
  return [tab, choose];
}

export function SettingsTabBar({ tab, onChange }: { tab: SettingsTab; onChange: (tab: SettingsTab) => void }) {
  const ids = availableTabs(IS_MOBILE);
  const tabs = SETTINGS_TABS.filter((t) => ids.includes(t.id));
  const buttons = useRef(new Map<SettingsTab, HTMLButtonElement>());
  // Arrow keys, Home and End move along the tabs, and open the one they reach.
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const at = ids.indexOf(tab);
    const to =
      e.key === "ArrowRight" || e.key === "ArrowDown" ? (at + 1) % ids.length
      : e.key === "ArrowLeft" || e.key === "ArrowUp" ? (at - 1 + ids.length) % ids.length
      : e.key === "Home" ? 0
      : e.key === "End" ? ids.length - 1
      : -1;
    if (to < 0) return;
    e.preventDefault();
    onChange(ids[to]);
    buttons.current.get(ids[to])?.focus();
  };
  return (
    <div role="tablist" aria-label="Settings" onKeyDown={onKeyDown} className="flex flex-wrap gap-2 mb-6 sm:mb-8">
      {tabs.map(({ id, label }) => {
        const Icon = ICONS[id];
        const on = id === tab;
        return (
          <button
            key={id}
            ref={(el) => { if (el) buttons.current.set(id, el); else buttons.current.delete(id); }}
            type="button"
            role="tab"
            id={tabId(id)}
            aria-selected={on}
            aria-controls={PANEL_ID}
            tabIndex={on ? 0 : -1}
            onClick={() => onChange(id)}
            className={`inline-flex items-center gap-2 px-3.5 h-9 rounded-xl border text-xs font-bold uppercase tracking-wider whitespace-nowrap transition-all outline-none focus-visible:ring-2 focus-visible:ring-primary/50 ${
              on ? "bg-primary/10 border-primary/30 text-primary" : "bg-zinc-900 border-white/5 text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
            }`}
          >
            <Icon size={14} /> {label}
          </button>
        );
      })}
    </div>
  );
}

/** Around the original project's cards: only those under the open tab show
 *  (a card with a heading the fork doesn't know shows under General), and
 *  ForkSettings, which picks its own. A `contents` box, so the cards stay in
 *  the page's columns. */
export function InTab({ tab, children }: { tab: SettingsTab; children: ReactNode }) {
  const shown = Children.toArray(children).filter(
    (card) => (isValidElement(card) && card.type === ForkSettings) || tabForHeading(headingOf(card)) === tab,
  );
  return (
    <div role="tabpanel" id={PANEL_ID} aria-labelledby={tabId(tab)} className="contents">
      {shown}
    </div>
  );
}

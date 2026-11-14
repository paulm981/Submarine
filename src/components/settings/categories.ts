// Settings in categories (#168): the tabs across the top of Settings, which
// of the original project's cards goes under which, and the tab this device
// last had open. No React here, so the tests can reach all of it.
//
// The original project's cards stay in their file as they wrote them; the
// fork tells them apart by their heading (`headingOf`), so a sync needs no
// change to each card. A card whose heading isn't listed below, one they
// add or rename, shows under General: never lost, only maybe misplaced
// (tools/test/settings.test.ts names it).

export type SettingsTab = "general" | "terminal" | "servers" | "ai" | "sync" | "help";

export const SETTINGS_TABS: readonly { id: SettingsTab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "terminal", label: "Terminal" },
  { id: "servers", label: "Servers & sessions" },
  { id: "ai", label: "AI tools" },
  { id: "sync", label: "Sync" },
  { id: "help", label: "Help" },
];

/** The original project's cards in SettingsPanel.tsx, by their heading. */
export const UPSTREAM_CARD_TABS: Readonly<Record<string, SettingsTab>> = {
  "UI Customization": "general",
  "Privacy": "general",
  "Activity Log": "general",
  "Maintenance": "general",
  "Terminal Configuration": "terminal",
  "Authentication": "terminal",
  "Connections": "servers",
  "Cloud Sync": "sync",
  "Help & feedback": "help",
};

/** The original project's AI settings (#252): components of their own that
 *  SettingsPanel renders, "AI Assistant" (ai/AiSettings.tsx) and "AI agents"
 *  (agents/AgentSettings.tsx), not cards written there, so SettingsTabs knows
 *  them by what they are (AI_SETTINGS). They go under AI tools; where that
 *  tab isn't shown (a phone), under General, as the assistant works there. */
export function tabForAiSettings(available: readonly SettingsTab[]): SettingsTab {
  return available.includes("ai") ? "ai" : "general";
}

/** The tab a card with this heading goes under: General when it's unknown.
 *  In another language a card shows its heading translated, so `translate`
 *  (the page's t) turns each English heading above into the one on screen. */
export function tabForHeading(heading: string | null, translate: (english: string) => string = (s) => s): SettingsTab {
  if (heading === null) return "general";
  if (Object.prototype.hasOwnProperty.call(UPSTREAM_CARD_TABS, heading)) return UPSTREAM_CARD_TABS[heading];
  const english = Object.keys(UPSTREAM_CARD_TABS).find((k) => translate(k) === heading);
  return english ? UPSTREAM_CARD_TABS[english] : "general";
}

/** The tabs this device shows: AI tools only where they run (not on a
 *  phone or tablet, where Settings has none of them). */
export function availableTabs(mobile: boolean): SettingsTab[] {
  return SETTINGS_TABS.map((t) => t.id).filter((id) => !(mobile && id === "ai"));
}

/** A remembered tab, as stored: one of `available`, or General. */
export function parseTab(raw: unknown, available: readonly SettingsTab[]): SettingsTab {
  return typeof raw === "string" && (available as readonly string[]).includes(raw) ? (raw as SettingsTab) : "general";
}

export const SETTINGS_TAB_KEY = "submarine-settings-tab";

/** The tab this device last had open. Storage can be missing or refuse. */
export function readTab(available: readonly SettingsTab[]): SettingsTab {
  try {
    return parseTab(localStorage.getItem(SETTINGS_TAB_KEY), available);
  } catch {
    return "general";
  }
}

export function writeTab(tab: SettingsTab): void {
  try {
    localStorage.setItem(SETTINGS_TAB_KEY, tab);
  } catch {
    /* not remembered: the next visit opens General */
  }
}

// A React element, as far as reading its heading needs: no React import, so
// it can be tested with plain objects.
type ElementLike = { type: unknown; props: { children?: unknown } };

const isElementLike = (n: unknown): n is ElementLike =>
  typeof n === "object" && n !== null && "type" in n && typeof (n as { props?: unknown }).props === "object" && (n as { props?: unknown }).props !== null;

const textOf = (n: unknown): string =>
  typeof n === "string" || typeof n === "number"
    ? String(n)
    : Array.isArray(n)
      ? n.map(textOf).join("")
      : isElementLike(n)
        ? textOf(n.props.children)
        : "";

/** A card's heading: the text of the first element in a `<section>`, as
 *  the original project writes each card (`<Icon /> Heading`). Null for
 *  anything else. */
export function headingOf(card: unknown): string | null {
  if (!isElementLike(card) || card.type !== "section") return null;
  const kids = card.props.children;
  const first = (Array.isArray(kids) ? kids : [kids]).find(isElementLike);
  const text = first ? textOf(first.props.children).replace(/\s+/g, " ").trim() : "";
  return text || null;
}

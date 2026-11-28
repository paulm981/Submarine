// Lightweight highlight preferences and constants kept out of the initial
// startup bundle so `ui/highlight.ts` (and `re2js`) only load lazily after
// the profile picker (#285).

export const HIGHLIGHT_KEY = "submarine-terminal-highlight";
export const HIGHLIGHT_NONE = "none";
export const HIGHLIGHT_MINE = "mine";
export const DEFAULT_HIGHLIGHT = "standard";

export const HIGHLIGHT_SET_IDS: ReadonlySet<string> = new Set([
  HIGHLIGHT_NONE,
  "standard",
  "shell",
  "network",
  "sql",
  HIGHLIGHT_MINE,
]);

/** This device's set (Settings › Terminal). */
export function readHighlightId(): string {
  try {
    const id = localStorage.getItem(HIGHLIGHT_KEY) || DEFAULT_HIGHLIGHT;
    return HIGHLIGHT_SET_IDS.has(id) ? id : DEFAULT_HIGHLIGHT;
  } catch {
    return DEFAULT_HIGHLIGHT;
  }
}

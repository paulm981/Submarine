import { useSyncExternalStore } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Terminal activity — "this terminal printed something while you weren't
// looking", shown as a dot on its tab and on its server's tab.
//
// Output already reaches every terminal while it is hidden (TerminalView keeps
// them all mounted), so this only listens on that same path: TerminalView
// reports each chunk (`noteOutput`) and whether the terminal is on screen
// (`setLooking`). A terminal on screen — in its tab, in a split, or in a Wall
// that is showing — never has news; looking at it clears it.
// ─────────────────────────────────────────────────────────────────────────────

/// Output in this long after you leave a terminal is ignored: the prompt
/// redraw, the resize that comes with switching tabs, the tail of what you
/// were just watching.
const QUIET_AFTER_LEAVING_MS = 1000;
/// A chunk needs this many visible characters to count. A lone cursor move,
/// a title change or a prompt's colour reset has none; a real line of output
/// (even "ok\n" plus something) has more.
const MIN_VISIBLE_CHARS = 3;

interface Entry { sessionId: string; looking: boolean; leftAt: number; news: boolean }
const entries = new Map<string, Entry>();
/// How many terminals of each session have news, so a server tab's dot
/// doesn't walk every terminal to find out.
const newsPerSession = new Map<string, number>();
const listeners = new Set<() => void>();

const setNews = (e: Entry, news: boolean) => {
  if (e.news === news) return;
  e.news = news;
  const n = (newsPerSession.get(e.sessionId) ?? 0) + (news ? 1 : -1);
  if (n > 0) newsPerSession.set(e.sessionId, n);
  else newsPerSession.delete(e.sessionId);
  for (const l of listeners) l();
};

// Escape sequences (CSI, OSC up to BEL or ST, and two-byte ones), then
// control characters and spaces: what's left is text someone could read.
// eslint-disable-next-line no-control-regex
const ESCAPES = /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[@-Z\\-_]|\x1b[ -/]+[0-~]/g;
// eslint-disable-next-line no-control-regex
const NOT_VISIBLE = /[\x00-\x20\x7f]/g;

/// True when `text` has real output in it, not just terminal control codes.
export const hasVisibleText = (text: string): boolean =>
  text.replace(ESCAPES, "").replace(NOT_VISIBLE, "").length >= MIN_VISIBLE_CHARS;

// One decoder for every chunk. It isn't in streaming mode, so a UTF-8
// character split between two chunks comes out as U+FFFD on each side — fine
// here, as it only has to tell whether there is something to read. (It
// never throws: `fatal` is off.)
const decoder = new TextDecoder();

/// A terminal opened (its output starts being watched). It begins as "just
/// left", so its first second of output (login banner) isn't news.
export const watchTerminal = (terminalId: string, sessionId: string): (() => void) => {
  const e: Entry = { sessionId, looking: false, leftAt: Date.now(), news: false };
  entries.set(terminalId, e);
  return () => {
    if (entries.get(terminalId) !== e) return;
    entries.delete(terminalId);
    setNews(e, false);
  };
};

/// The terminal is now on screen (or not).
export const setLooking = (terminalId: string, looking: boolean) => {
  const e = entries.get(terminalId);
  if (!e || e.looking === looking) return;
  e.looking = looking;
  if (looking) setNews(e, false);
  else e.leftAt = Date.now();
};

/// A chunk of output arrived for the terminal.
export const noteOutput = (terminalId: string, data: Uint8Array) => {
  const e = entries.get(terminalId);
  if (!e || e.looking || e.news) return;
  if (Date.now() - e.leftAt < QUIET_AFTER_LEAVING_MS) return;
  if (!hasVisibleText(decoder.decode(data))) return;
  setNews(e, true);
};

export const terminalHasNews = (terminalId: string): boolean => entries.get(terminalId)?.news ?? false;
export const sessionHasNews = (sessionId: string): boolean => newsPerSession.has(sessionId);

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

/// True while any of these terminals, or any terminal of these sessions, has
/// news. The caller re-renders only when that answer changes, not on every
/// other terminal's news — with 80 servers open, one dot changing must not
/// redraw them all.
export const useHasNews = ({ terminals = [], sessions = [] }: { terminals?: readonly string[]; sessions?: readonly string[] }): boolean =>
  useSyncExternalStore(subscribe, () => terminals.some(terminalHasNews) || sessions.some(sessionHasNews));

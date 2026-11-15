import type { Dispatch, SetStateAction } from "react";
import type { SessionTerminal } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's terminal split in a server's tab (#163): what SessionView's
// buttons and menus do to it. Plain functions of SessionView's state, made
// on each render, as SessionView made them.
//
// The split stays as it was while a tab outside it is in front (splitShown,
// in ./rules), and comes back when one of its tabs is picked again. A pane
// that leaves it stays open as a tab; with one pane left the split ends.
// ─────────────────────────────────────────────────────────────────────────────

export const splitActions = ({
  sessionId, splitOn, splitTerminals, setSplitTerminals, setSplitRatios, activeTab, setActiveTab,
  setTerminals, nextTerminalNumber, ownLogin,
}: {
  sessionId: string;
  /** The split is on screen (splitShown). */
  splitOn: boolean;
  splitTerminals: string[];
  setSplitTerminals: Dispatch<SetStateAction<string[]>>;
  setSplitRatios: Dispatch<SetStateAction<number[]>>;
  activeTab: string;
  setActiveTab: Dispatch<SetStateAction<string>>;
  setTerminals: Dispatch<SetStateAction<SessionTerminal[]>>;
  /** SessionView's: the title of a new terminal. */
  nextTerminalNumber: (terminals: SessionTerminal[]) => string;
  /** New terminals of this server log in on their own. */
  ownLogin: boolean;
}) => {
  // Show `id` beside what's on screen: added to the split in front, or a new
  // split of the tab in front and `id`.
  const splitWith = (id: string) => {
    if (splitOn) {
      if (splitTerminals.includes(id)) return;
      setSplitTerminals(prev => [...prev, id]);
      setSplitRatios(prev => [...prev, 1]);
    } else if (id !== activeTab) {
      setSplitTerminals([activeTab, id]);
      setSplitRatios([1, 1]);
    }
  };
  // Another shell on this server, beside the one(s) on screen.
  const splitWithNew = () => {
    const newId = `${sessionId}-term-${Date.now()}`;
    setTerminals(prev => [...prev, { id: newId, title: nextTerminalNumber(prev), ownLogin }]);
    splitWith(newId);
    setActiveTab(newId);
  };
  // A pane leaves the split and stays open as a tab; with one pane left the
  // split ends.
  const removeFromSplit = (id: string) => {
    const idx = splitTerminals.indexOf(id);
    if (idx < 0) return;
    const nextTerms = splitTerminals.filter(x => x !== id);
    if (nextTerms.length < 2) {
      setSplitTerminals([]);
      setSplitRatios([]);
    } else {
      setSplitTerminals(nextTerms);
      setSplitRatios(prev => prev.filter((_, i) => i !== idx));
    }
    if (activeTab === id) setActiveTab(nextTerms[0] ?? id);
  };
  return { splitWith, splitWithNew, removeFromSplit };
};
export type SplitActions = ReturnType<typeof splitActions>;

/** The window event asking the main window to close a server's tab. */
export const CLOSE_TAB_ASKING = "submarine-close-tab-asking";
/** Ctrl+Shift+W on a tab's last terminal (the original project's
 *  closeTerminal, #340) closes the tab the way its × in the title bar does:
 *  through the fork's close (useCloseSessions), which asks first while it's
 *  connected ("Close this session?", unless that's turned off) and drops its
 *  Wall pin. False when nothing took it (no main window listening), and the
 *  caller then closes it the original project's way. */
export const closeTabAsking = (sessionId: string): boolean =>
  !window.dispatchEvent(new CustomEvent(CLOSE_TAB_ASKING, { detail: sessionId, cancelable: true }));

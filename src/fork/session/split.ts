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
  terminals, setTerminals, nextTerminalNumber, ownLogin,
}: {
  sessionId: string;
  /** The split is on screen (splitShown). */
  splitOn: boolean;
  splitTerminals: string[];
  setSplitTerminals: Dispatch<SetStateAction<string[]>>;
  setSplitRatios: Dispatch<SetStateAction<number[]>>;
  activeTab: string;
  setActiveTab: Dispatch<SetStateAction<string>>;
  terminals: SessionTerminal[];
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
  // A terminal tab's close button.
  const closeTerminal = (id: string) => {
    const rest = terminals.filter(x => x.id !== id);
    setTerminals(rest);
    // A closed terminal leaves the split too, or the layout
    // would keep a slot for it.
    removeFromSplit(id);
    // The one in front closed: the split's next pane, else the
    // first tab left (not the closed one, when it was first).
    if (activeTab === id) {
      const nextInSplit = splitTerminals.find(s => s !== id && rest.some(x => x.id === s));
      setActiveTab(nextInSplit ?? rest[0].id);
    }
  };
  return { splitWith, splitWithNew, removeFromSplit, closeTerminal };
};
export type SplitActions = ReturnType<typeof splitActions>;

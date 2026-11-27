// The fork's side of the original project's keyboard shortcuts table (#374,
// util/shortcuts; Settings → Keyboard shortcuts). Their files reach this in
// one-line hooks (#163), checked by tools/test/fork-hooks.test.ts:
//   - mayBeShortcut: the early return for plain typing, before their table is
//     looked up, in DesktopApp's window listener and TerminalView's key handler;
//   - tabStepList: Next tab and Previous tab (Ctrl+Tab, Ctrl+Shift+Tab) go
//     through the title bar as the fork shows it;
//   - RESERVED_KEYS and ReservedKeysNote: the fork's keys beside the table,
//     which it won't let an action take.
import { comboTextLabel, readShortcuts, type ShortcutTable } from "../util/shortcuts";

// What the table holds that a key without Ctrl or ⌘ could match: a key set
// with Alt alone (Alt+B, say), or an F-key set with no modifier but Shift.
// (Insert is always let through, below.) Worked out again only when the
// table changes: readShortcuts gives the same object until then.
let seen: ShortcutTable | null = null;
let altOnly = false;
let fKeys = false;
const reread = (table: ShortcutTable) => {
  seen = table;
  altOnly = false;
  fKeys = false;
  for (const combos of Object.values(table)) {
    for (const c of combos) {
      if (c.ctrl || c.meta) continue;
      if (c.alt) altOnly = true;
      else if (c.key !== "Insert") fKeys = true;
    }
  }
};

/** Whether a key event can be one of the table's shortcuts at all, so plain
 *  typing goes on before anything is looked up or made. Their rule
 *  (usableCombo): a shortcut holds Ctrl, Alt or ⌘, except F1 to F24 and
 *  Shift+Insert. Nearly all typing has none of those. Keys typed with Alt
 *  alone (a Mac's Option types @ and [ on many layouts) and F-keys go on too
 *  unless the table has a key of that kind, which it doesn't by default.
 *  Insert always counts: a paste key the table doesn't have is held back
 *  there (isWebviewEditKey), whatever the table holds. A few comparisons, no
 *  regex, nothing allocated: it runs on every key typed. */
export const mayBeShortcut = (e: KeyboardEvent): boolean => {
  if (e.ctrlKey || e.metaKey) return true;
  const k = e.key;
  if (k === "Insert") return true;
  if (e.altKey) {
    const table = readShortcuts();
    if (table !== seen) reread(table);
    return altOnly;
  }
  // "F1" to "F24": an F, then a digit 1 to 9. No other key's name starts so.
  // (A browser's autofill sends a keydown with no key at all.)
  if (!k || k.length < 2 || k.charCodeAt(0) !== 70) return false;
  const d = k.charCodeAt(1);
  if (d < 49 || d > 57) return false;
  const table = readShortcuts();
  if (table !== seen) reread(table);
  return fKeys;
};

/** The fork's keys, which stay beside the table, written as the table keeps
 *  keys (util/shortcuts' comboText). Ctrl+Shift+J jumps to a tab by name,
 *  Ctrl+Shift+L opens or folds the AI side pane (ui/terminalDock's
 *  appHotkey), and Ctrl+Shift+P (or ⇧⌘P) pauses or resumes a Broadcast tile
 *  (fork/desktop/broadcastTiles). The fork takes them before the table's
 *  handlers would, so the table refuses them: an action given one would show
 *  a key that does nothing. */
export const RESERVED_KEYS: readonly string[] = ["Ctrl+Shift+J", "Ctrl+Shift+L", "Ctrl+Shift+P", "Shift+Meta+P"];

/** The line under the table that names the fork's keys. */
export function ReservedKeysNote() {
  const keys = (text: string) => <kbd className="font-mono text-zinc-400">{comboTextLabel(text)}</kbd>;
  return (
    <p className="text-[11px] text-zinc-500 leading-relaxed" data-fork-reserved-keys="">
      These stay as they are: {keys("Ctrl+Shift+J")} jumps to a tab by name, {keys("Ctrl+Shift+L")} opens the AI side
      pane, and {keys("Ctrl+Shift+P")} pauses a Broadcast tile.
    </p>
  );
}

// The title bar's tabs, left to right, as the fork shows them: Home, the
// Broadcast tab while it's on, then the server tabs not folded into it.
// useTabKeys sets it as it renders.
let stepOrder: readonly string[] | null = null;
export const setTabStepOrder = (order: readonly string[]) => { stepOrder = order; };

/** The tabs Next tab and Previous tab step through, in the title bar's order
 *  (their handler in DesktopApp, given its own list of server tabs: the one
 *  used before the title bar has drawn). Only on those keys, never per key
 *  typed. */
export const tabStepList = (sessions: readonly { id: string }[]): { id: string }[] =>
  stepOrder ? stepOrder.map((id) => ({ id })) : [...sessions];

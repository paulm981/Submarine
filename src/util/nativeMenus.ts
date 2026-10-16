// The webview's own right-click menu and reload keys, turned off.
//
// A right-click on any spot without a menu of the app's own used to open the
// webview's menu, and its "Reload" restarts the whole app: every session and
// terminal gone, with nothing to undo it. Now that menu opens only where it
// helps: in a text field (cut, copy, paste) and on selected text (copy). F5
// and Ctrl+R are taken the same way outside a terminal; in a terminal they
// have already gone to the shell (Ctrl+R is its history search) before this
// sees them. A development build keeps the reload keys, and Shift+right-click
// opens the full menu there (Inspect Element).

const editable = (el: Element | null) =>
  !!el?.closest("input, textarea, [contenteditable=''], [contenteditable='true']");

// Selected text under the pointer: the menu's Copy is for it. A right-click
// elsewhere, with text selected somewhere else, gets the page menu (Reload).
const onSelection = (target: EventTarget | null) => {
  const sel = window.getSelection();
  if (!sel || !sel.toString().trim() || !(target instanceof Node)) return false;
  return sel.containsNode(target, true);
};

export const tameNativeMenus = () => {
  // After the app's own handlers (React's sit on the root, below the
  // document), so a menu of its own has already called preventDefault.
  document.addEventListener("contextmenu", (e) => {
    if (e.defaultPrevented || (import.meta.env.DEV && e.shiftKey)) return;
    if (editable(e.target as Element | null) || onSelection(e.target)) return;
    e.preventDefault();
  });
  if (import.meta.env.DEV) return;
  window.addEventListener("keydown", (e) => {
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
    if (e.key === "F5" || (ctrl && e.key.toLowerCase() === "r")) e.preventDefault();
  });
};

import { readClipboardText } from "./clipboard";

// The webview's own right-click menu and reload keys, replaced.
//
// A right-click on any spot without a menu of the app's own used to open the
// webview's menu, and its "Reload" restarts the whole app: every session and
// terminal gone, with nothing to undo it. That menu also comes in the
// system's light colors. So it never opens: in a text field the app shows its
// own (Cut, Copy, Paste, Select all), on selected text one with Copy, and
// elsewhere nothing unless the spot has a menu of its own. F5 and Ctrl+R are
// taken the same way outside a terminal; in a terminal they have already gone
// to the shell (Ctrl+R is its history search) before this sees them. A
// development build keeps the reload keys, and Shift+right-click opens the
// webview's menu there (Inspect Element).

type Field = HTMLInputElement | HTMLTextAreaElement;

const TEXT_INPUTS = new Set(["text", "search", "url", "tel", "email", "password", "number", ""]);

const textField = (el: EventTarget | null): Field | null => {
  if (el instanceof HTMLTextAreaElement) return el;
  if (el instanceof HTMLInputElement && TEXT_INPUTS.has(el.type)) return el;
  return null;
};

// Selected text under the pointer, outside a text field.
const selectionUnder = (target: EventTarget | null): string | null => {
  const sel = window.getSelection();
  const text = sel?.toString() ?? "";
  if (!sel || !text.trim() || !(target instanceof Node) || !sel.containsNode(target, true)) return null;
  return text;
};

type Item = { label: string; enabled: boolean; run: () => void };

let openMenu: { el: HTMLDivElement; close: () => void } | null = null;

const showMenu = (x: number, y: number, items: Item[]) => {
  openMenu?.close();
  const menu = document.createElement("div");
  menu.setAttribute("role", "menu");
  menu.className =
    "fixed z-[10000] min-w-[160px] bg-[#0c0c0e] border border-white/10 rounded-lg shadow-2xl p-1 text-[12px] text-zinc-200 select-none";

  const onDown = (e: MouseEvent) => {
    if (!menu.contains(e.target as Node)) close();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };
  const close = () => {
    menu.remove();
    window.removeEventListener("mousedown", onDown, true);
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("blur", close);
    window.removeEventListener("resize", close);
    window.removeEventListener("wheel", close, true);
    if (openMenu?.el === menu) openMenu = null;
  };

  for (const item of items) {
    const b = document.createElement("button");
    b.type = "button";
    b.setAttribute("role", "menuitem");
    b.textContent = item.label;
    b.disabled = !item.enabled;
    b.className =
      "w-full text-left px-2.5 py-1.5 rounded hover:bg-white/10 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent disabled:cursor-default";
    // Keeps the focus (and the selection) in the field the menu is for.
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => {
      close();
      item.run();
    });
    menu.appendChild(b);
  }
  document.body.appendChild(menu);
  // At the pointer, moved up or left as far as it must to stay in the window.
  const { width, height } = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - height - 4))}px`;

  window.addEventListener("mousedown", onDown, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", close);
  window.addEventListener("resize", close);
  window.addEventListener("wheel", close, true);
  openMenu = { el: menu, close };
};

const copy = (text: string) => {
  navigator.clipboard.writeText(text).catch(() => {});
};

const fieldMenu = (field: Field): Item[] => {
  const start = field.selectionStart ?? 0;
  const end = field.selectionEnd ?? 0;
  const selected = field.value.slice(start, end);
  const editable = !field.readOnly && !field.disabled;
  // A password field's text is never copied out of it, as browsers do.
  const secret = field instanceof HTMLInputElement && field.type === "password";
  // Typed through execCommand, so the change is the field's own: React sees
  // it, and Ctrl+Z undoes it.
  const insert = (text: string) => {
    field.focus();
    field.setSelectionRange(start, end);
    if (!document.execCommand("insertText", false, text)) {
      field.setRangeText(text, start, end, "end");
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
  };
  return [
    {
      label: "Cut",
      enabled: editable && !secret && selected.length > 0,
      run: () => {
        copy(selected);
        insert("");
      },
    },
    { label: "Copy", enabled: !secret && selected.length > 0, run: () => copy(selected) },
    {
      label: "Paste",
      enabled: editable,
      run: () => {
        readClipboardText().then(
          (text) => {
            if (text) insert(text);
          },
          () => {},
        );
      },
    },
    {
      label: "Select all",
      enabled: field.value.length > 0,
      run: () => {
        field.focus();
        field.select();
      },
    },
  ];
};

export const tameNativeMenus = () => {
  // After the app's own handlers (React's sit on the root, below the
  // document), so a menu of its own has already called preventDefault.
  document.addEventListener("contextmenu", (e) => {
    if (e.defaultPrevented) return;
    if (import.meta.env.DEV && e.shiftKey) return;
    e.preventDefault();
    const field = textField(e.target);
    if (field) {
      showMenu(e.clientX, e.clientY, fieldMenu(field));
      return;
    }
    const text = selectionUnder(e.target);
    if (text) showMenu(e.clientX, e.clientY, [{ label: "Copy", enabled: true, run: () => copy(text) }]);
  });
  if (import.meta.env.DEV) return;
  window.addEventListener("keydown", (e) => {
    const ctrl = (e.ctrlKey || e.metaKey) && !e.altKey;
    if (e.key === "F5" || (ctrl && e.key.toLowerCase() === "r")) e.preventDefault();
  });
};

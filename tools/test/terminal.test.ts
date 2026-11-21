// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).
// The fork's terminal and tab helpers that decide without a page.
import { describe, expect, it } from "vitest";
import { claimGpu, gpuChanged, gpuSlotsInUse, MAX_GPU, releaseGpu, type GpuHolder } from "../../src/ui/gpuSlots";
import { reorderById } from "../../src/ui/tabDrag";
import { hasVisibleText } from "../../src/ui/terminalActivity";
import { cdLine } from "../../src/fork/terminal/here";
import { pathLike, resolvePath, wordAt } from "../../src/fork/terminal/fileMenu";
import { MOST_PER_FRAME, nextTurns, QUICK_FRAME_MS, SLOW_FRAME_MS } from "../../src/fork/terminal/turns";
import { clearWidths, MAX_WIDTHS, sharedMeasure, widthKey } from "../../src/fork/terminal/widthCache";
import { CUSTOM_SCHEME, DEFAULT_SCHEME, schemeById } from "../../src/ui/terminalThemes";
import { CUSTOM_THEME_ID, readScheme } from "../../src/util/terminalTheme";
import { CLOSE_TAB_ASKING, closeTabAsking } from "../../src/fork/session/split";
import { appHotkey } from "../../src/ui/terminalDock";
import { mayBeShortcut, RESERVED_KEYS } from "../../src/fork/keys";
import {
  comboOf, comboText, defaultShortcuts, isWebviewEditKey, parseCombo, resetShortcuts, setShortcuts, SHORTCUT_ACTIONS, shortcutFor, usableCombo,
} from "../../src/util/shortcuts";

describe("hasVisibleText", () => {
  it("ignores control codes: colours, a title change, cursor moves", () => {
    expect(hasVisibleText("\x1b[0m\x1b]0;user@host: ~\x07\x1b[2J\x1b[H")).toBe(false);
    expect(hasVisibleText("\x1b]2;title\x1b\\\r\n")).toBe(false);
  });

  it("needs a few visible characters, not a lone prompt redraw", () => {
    expect(hasVisibleText("$ ")).toBe(false);
    expect(hasVisibleText(" ok\r\n")).toBe(false);
    expect(hasVisibleText("\x1b[32mdone\x1b[0m\r\n")).toBe(true);
  });
});

describe("reorderById", () => {
  const tabs = ["a", "b", "c", "d"].map((id) => ({ id }));
  const ids = (list: { id: string }[]) => list.map((t) => t.id).join("");

  it("moves a tab before or after another", () => {
    expect(ids(reorderById(tabs, "a", "c", false))).toBe("bacd");
    expect(ids(reorderById(tabs, "a", "c", true))).toBe("bcad");
    expect(ids(reorderById(tabs, "d", "a", false))).toBe("dabc");
  });

  it("leaves the list as it is for a drop on itself or an unknown tab", () => {
    expect(reorderById(tabs, "b", "b", true)).toBe(tabs);
    expect(reorderById(tabs, "x", "b", true)).toBe(tabs);
    expect(reorderById(tabs, "b", "x", true)).toBe(tabs);
  });
});

describe("gpuSlots", () => {
  const holder = (onScreen: boolean) => {
    const h = { screen: onScreen, evicted: 0, retried: 0 } as { screen: boolean; evicted: number; retried: number } & GpuHolder;
    h.onScreen = () => h.screen;
    h.evict = () => { h.evicted++; };
    h.retry = () => { h.retried++; };
    return h;
  };

  it("hands out at most MAX_GPU contexts, takes one back from a terminal off screen, and retries waiting ones", () => {
    const held = Array.from({ length: MAX_GPU }, () => holder(true));
    for (const h of held) expect(claimGpu(h)).toBe(true);
    expect(claimGpu(held[0])).toBe(true); // already holding: no change

    // All on screen: the next one waits.
    const late = holder(true);
    expect(claimGpu(late)).toBe(false);
    expect(gpuSlotsInUse()).toEqual({ holding: MAX_GPU, waiting: 1 });

    // One goes off screen: the next claim takes its context.
    held[3].screen = false;
    const next = holder(true);
    expect(claimGpu(next)).toBe(true);
    expect(held[3].evicted).toBe(1);

    // One closes: the terminal on screen that waited is told to try again.
    releaseGpu(held[0]);
    expect(late.retried).toBe(1);
    expect(claimGpu(late)).toBe(true);

    // Nothing waits now: a change wakes nobody.
    gpuChanged();
    expect(late.retried).toBe(1);
    for (const h of [...held, late, next]) releaseGpu(h);
    expect(gpuSlotsInUse()).toEqual({ holding: 0, waiting: 0 });
  });
});

describe("terminals opened a few at a time (#127)", () => {
  it("halves the turns after a slow frame and adds one after a quick one, within bounds", () => {
    expect(nextTurns(4, SLOW_FRAME_MS + 1)).toBe(2);
    expect(nextTurns(1, 500)).toBe(1);
    expect(nextTurns(2, QUICK_FRAME_MS - 1)).toBe(3);
    expect(nextTurns(MOST_PER_FRAME, 1)).toBe(MOST_PER_FRAME);
    expect(nextTurns(3, (SLOW_FRAME_MS + QUICK_FRAME_MS) / 2)).toBe(3);
  });
});

describe("character widths shared by every terminal (#127)", () => {
  const cache = (over: Partial<{ _font: string; _fontSize: number; _weight: string; _weightBold: string }> = {}) =>
    ({ _font: "mono", _fontSize: 14, _weight: "normal", _weightBold: "bold", ...over });
  (globalThis as { window?: unknown }).window = { devicePixelRatio: 1 };

  it("measures a character once for every terminal in the same font", () => {
    clearWidths();
    let measured = 0;
    const measure = sharedMeasure(() => { measured++; return 8.4; });
    expect(measure.call(cache(), "W", 0)).toBe(8.4);
    expect(measure.call(cache(), "W", 0)).toBe(8.4);
    expect(measured).toBe(1);
    // Another size, weight or style is measured on its own.
    measure.call(cache({ _fontSize: 16 }), "W", 0);
    measure.call(cache(), "W", 1);
    measure.call(cache(), "W", 2);
    expect(measured).toBe(4);
  });

  it("keeps no width of 0, from a terminal that isn't shown", () => {
    clearWidths();
    let width = 0;
    const measure = sharedMeasure(() => width);
    expect(measure.call(cache(), "x", 0)).toBe(0);
    width = 7;
    expect(measure.call(cache(), "x", 0)).toBe(7);
  });

  it("tells bold from regular by the cache's own weights, and italic apart", () => {
    const c = cache({ _weight: "400", _weightBold: "700" });
    expect(widthKey(c, "a", 0, 1)).not.toBe(widthKey(c, "a", 1, 1));
    expect(widthKey(c, "a", 1, 1)).not.toBe(widthKey(c, "a", 3, 1));
    expect(widthKey(c, "a", 0, 1)).not.toBe(widthKey(c, "a", 0, 2));
  });

  it("keeps widths apart by weight, so one terminal's bold isn't another's", () => {
    expect(widthKey(cache({ _weightBold: "700" }), "a", 1, 1)).not.toBe(widthKey(cache({ _weightBold: "900" }), "a", 1, 1));
    expect(widthKey(cache({ _weight: "300" }), "a", 2, 1)).not.toBe(widthKey(cache({ _weight: "400" }), "a", 2, 1));
    // A weight a variant doesn't use doesn't split it.
    expect(widthKey(cache({ _weightBold: "700" }), "a", 0, 1)).toBe(widthKey(cache({ _weightBold: "900" }), "a", 0, 1));
  });

  it("finds what it reaches for in this xterm, so an update that renames it fails here", () => {
    const xterm = Object.values(import.meta.glob<string>("/node_modules/@xterm/xterm/lib/xterm.mjs", { query: "?raw", import: "default", eager: true }))[0];
    expect(xterm, "xterm's module").toBeTruthy();
    for (const name of ["_widthCache=new", "_measure(", "_weightBold", "_weight", "_fontSize", "_font"]) expect(xterm, name).toContain(name);
  });

  it("starts again past its limit rather than growing", () => {
    clearWidths();
    let measured = 0;
    const measure = sharedMeasure(() => { measured++; return 1; });
    for (let i = 0; i <= MAX_WIDTHS; i++) measure.call(cache(), `c${i}`, 0);
    measured = 0;
    measure.call(cache(), "c0", 0);
    expect(measured).toBe(1);
  });
});

describe("Open terminal here (#72)", () => {
  it("goes to a server's folder, quoted", () => {
    expect(cdLine("/var/log")).toBe("cd '/var/log'");
    expect(cdLine("/srv/my app")).toBe("cd '/srv/my app'");
    expect(cdLine("/srv/it's")).toBe("cd '/srv/it'\\''s'");
    expect(cdLine("/srv/$(reboot);x`y`")).toBe("cd '/srv/$(reboot);x`y`'");
  });

  it("isn't offered where a shell could read the name as more", () => {
    for (const path of ["relative", "/a\nreboot", "/a\rb", "/a\u001b[31m", "/a\u0085b", "/x!rm", "/a\\b", "/C:/Users", "/" + "x".repeat(5000)]) {
      expect(cdLine(path), path).toBeNull();
    }
  });
});

describe("right-click on a file name (#40)", () => {
  const at = (line: string, needle: string) => wordAt(line, line.indexOf(needle) + 1);
  it("takes the word under the pointer, without quotes, brackets or a line number", () => {
    expect(at("-rw-r--r-- 1 root root 812 nginx.conf", "nginx.conf")).toBe("nginx.conf");
    expect(at("error in src/main.rs:12:5: oops", "src/main.rs")).toBe("src/main.rs");
    expect(at('open("/etc/hosts") failed', "/etc/hosts")).toBe("/etc/hosts");
    expect(at("see (logs/app.log).", "logs/app.log")).toBe("logs/app.log");
    expect(wordAt("a  b", 1)).toBe("");
  });

  it("knows a path or a file name from other words", () => {
    for (const w of ["/var/log/syslog", "nginx.conf", "./run.sh", "app.log", "logs/", ".env", "README.md"]) expect(pathLike(w), w).toBe(true);
    for (const w of ["uptime", "1.2.3.4", "0.21", "3.14", "https://example.com/a", "", "-", "x".repeat(5000)]) expect(pathLike(w), w).toBe(false);
  });

  it("resolves a name against the shell's folder, only when it can be sure", () => {
    expect(resolvePath("/etc/nginx/nginx.conf", undefined)).toBe("/etc/nginx/nginx.conf");
    expect(resolvePath("nginx.conf", "/etc/nginx")).toBe("/etc/nginx/nginx.conf");
    expect(resolvePath("./conf.d/a.conf", "/etc/nginx/")).toBe("/etc/nginx/conf.d/a.conf");
    expect(resolvePath("//etc//./hosts", undefined)).toBe("/etc/hosts");
    for (const [w, f] of [["nginx.conf", undefined], ["~/.bashrc", "/home/u"], ["../x.conf", "/etc/nginx"], ["/etc/a\nb", undefined]] as const) {
      expect(resolvePath(w, f), w).toBeNull();
    }
  });
});

describe("the original project's AI terminal (#252)", () => {
  it("takes this device's scheme from the fork's schemes", () => {
    // No storage here: the default scheme.
    expect(readScheme()).toBe(schemeById(DEFAULT_SCHEME));
    expect(readScheme().theme.background).toBeTruthy();
  });

  it("calls a pasted scheme by the same id in both files", () => {
    expect(CUSTOM_SCHEME).toBe(CUSTOM_THEME_ID);
  });
});

describe("Ctrl+Shift+W on a tab's last terminal (#275)", () => {
  // No page here: the window is a bare EventTarget for these tests.
  const g = globalThis as unknown as { window?: EventTarget };
  const withWindow = (run: (w: EventTarget) => void) => {
    const before = g.window;
    g.window = new EventTarget();
    try { run(g.window); } finally { g.window = before; }
  };

  it("goes to the main window's close, which asks first, and says it was taken", () => {
    withWindow((w) => {
      const asked: unknown[] = [];
      w.addEventListener(CLOSE_TAB_ASKING, (e) => { asked.push((e as CustomEvent).detail); e.preventDefault(); });
      expect(closeTabAsking("s1")).toBe(true);
      expect(asked).toEqual(["s1"]);
    });
  });

  it("says no one took it when no main window listens, so the tab closes the original project's way", () => {
    withWindow(() => { expect(closeTabAsking("s1")).toBe(false); });
  });
});

// The files' text, read by Vite (as fork-hooks.test.ts does).
const sources = import.meta.glob<string>(["/src/components/TerminalView.tsx", "/src/components/ShortcutSettings.tsx", "/src/fork/desktop/hooks.ts"], { query: "?raw", import: "default", eager: true });
const source = (path: string): string => sources[path] ?? "";

describe("Ctrl+Shift+K clears, Ctrl+Shift+J jumps to a tab (#275)", () => {
  const key = (code: string, mods: { ctrl?: boolean; shift?: boolean; meta?: boolean; alt?: boolean } = {}) => ({
    code, ctrlKey: !!mods.ctrl, shiftKey: !!mods.shift, metaKey: !!mods.meta, altKey: !!mods.alt,
  }) as KeyboardEvent;

  it("takes Ctrl+Shift+J for the jump, and leaves Ctrl+Shift+K to the terminal", () => {
    expect(appHotkey(key("KeyJ", { ctrl: true, shift: true }))).toBe("jump");
    expect(appHotkey(key("KeyK", { ctrl: true, shift: true }))).toBeNull();
    // Only with Ctrl and Shift, and nothing else.
    expect(appHotkey(key("KeyJ", { ctrl: true }))).toBeNull();
    expect(appHotkey(key("KeyJ", { ctrl: true, shift: true, alt: true }))).toBeNull();
    expect(appHotkey(key("KeyJ", { meta: true, shift: true }))).toBeNull();
  });

  it("has one handler for each: the terminal's clear on K, the fork's jump on J", () => {
    const terminal = source("/src/components/TerminalView.tsx");
    const tabKeys = source("/src/fork/desktop/hooks.ts");
    // The original project's shortcuts table (#374) clears on K (Ctrl+Shift+K,
    // ⇧⌘K on a Mac, as #277 had it), in the terminal's runShortcut, and has no J.
    expect(defaultShortcuts("clear").map(comboText)).toContain("Ctrl+Shift+K");
    expect(terminal).toMatch(/case 'clear':\s*if \(first\) clearTerminal\(terminalId\);/);
    expect(terminal).not.toMatch(/keyCode === 74|"KeyJ"|'KeyJ'/);
    // The jump goes only by appHotkey: no key of its own here.
    expect(tabKeys).toContain("const k = appHotkey(e);");
    expect(tabKeys).not.toMatch(/KeyK|KeyJ|keyCode === 7[45]/);
  });
});

// The original project's shortcuts table (#374) and the fork's keys beside it.
describe("the shortcuts table and the fork's keys", () => {
  const ev = (key: string, code: string, keyCode: number, m: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {}) => ({
    key, code, keyCode, isComposing: false, repeat: false,
    ctrlKey: !!m.ctrl, altKey: !!m.alt, shiftKey: !!m.shift, metaKey: !!m.meta,
    getModifierState: () => false,
  }) as unknown as KeyboardEvent;

  // Keys of every kind the table knows, with every set of modifiers.
  const KEYS: [string, string, number][] = [
    ...Array.from({ length: 26 }, (_, i) => [String.fromCharCode(97 + i), `Key${String.fromCharCode(65 + i)}`, 65 + i] as [string, string, number]),
    ...Array.from({ length: 10 }, (_, i) => [String(i), `Digit${i}`, 48 + i] as [string, string, number]),
    ...Array.from({ length: 24 }, (_, i) => [`F${i + 1}`, `F${i + 1}`, 112 + i] as [string, string, number]),
    ["+", "Equal", 187], ["=", "Equal", 187], ["-", "Minus", 189], ["/", "Slash", 191], ["[", "BracketLeft", 219],
    [" ", "Space", 32], ["Tab", "Tab", 9], ["Enter", "Enter", 13], ["Backspace", "Backspace", 8], ["Insert", "Insert", 45],
    ["Delete", "Delete", 46], ["ArrowUp", "ArrowUp", 38], ["Home", "Home", 36], ["PageDown", "PageDown", 34],
    ["Escape", "Escape", 27], ["Shift", "ShiftLeft", 16], ["Control", "ControlLeft", 17],
  ];
  const MODS = Array.from({ length: 16 }, (_, i) => ({ ctrl: !!(i & 1), alt: !!(i & 2), shift: !!(i & 4), meta: !!(i & 8) }));

  // What runShortcut acts on: a key set for an action (with a shell that has
  // ended, the close and new keys without Shift too), or a key the webview
  // would copy or paste with by itself.
  const used = (e: KeyboardEvent): boolean => {
    const c = comboOf(e);
    return !!c && (!!shortcutFor(c, SHORTCUT_ACTIONS, ["closeTerminal", "newTerminal"]) || isWebviewEditKey(c));
  };
  const everyUsedKeyGetsPast = () => {
    for (const [key, code, keyCode] of KEYS) {
      for (const m of MODS) {
        const e = ev(key, code, keyCode, m);
        if (used(e)) expect(mayBeShortcut(e), `${comboText(comboOf(e)!)}`).toBe(true);
      }
    }
  };

  it("lets every key the table acts on past the early return for plain typing", () => {
    everyUsedKeyGetsPast();
  });

  it("lets Alt and F-keys past only while the table has such a key", () => {
    // The table keeps its changes in localStorage, which Node doesn't have.
    const kept = new Map<string, string>();
    const was = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => { kept.set(k, v); }, removeItem: (k: string) => { kept.delete(k); } },
    });
    try {
      // By default none: Option typing @ or [ on a Mac, readline's Alt+B and
      // mc's F5 go straight on, as before the table.
      expect(mayBeShortcut(ev("@", "KeyL", 76, { alt: true }))).toBe(false);
      expect(mayBeShortcut(ev("b", "KeyB", 66, { alt: true }))).toBe(false);
      expect(mayBeShortcut(ev("F5", "F5", 116))).toBe(false);
      // Set one of each, and they get to the table.
      setShortcuts("nextTab", [parseCombo("Alt+B")!, parseCombo("F5")!]);
      setShortcuts("closeTerminal", [parseCombo("Alt+Shift+W")!]);
      expect(mayBeShortcut(ev("b", "KeyB", 66, { alt: true }))).toBe(true);
      expect(mayBeShortcut(ev("F5", "F5", 116))).toBe(true);
      everyUsedKeyGetsPast();
      // Back to the defaults: back to going straight on.
      resetShortcuts();
      expect(mayBeShortcut(ev("b", "KeyB", 66, { alt: true }))).toBe(false);
      expect(mayBeShortcut(ev("F5", "F5", 116))).toBe(false);
    } finally {
      resetShortcuts();
      if (was) Object.defineProperty(globalThis, "localStorage", was);
      else delete (globalThis as { localStorage?: unknown }).localStorage;
    }
  });

  it("stops plain typing at the early return: letters, digits, Shift, Enter, Backspace, the arrows", () => {
    const typed = KEYS.filter(([key]) => key !== "Insert");
    for (const [key, code, keyCode] of typed) {
      expect(mayBeShortcut(ev(key, code, keyCode)), key).toBe(false);
      expect(mayBeShortcut(ev(key.toUpperCase(), code, keyCode, { shift: true })), `Shift+${key}`).toBe(false);
    }
    // A capital F is a letter, not an F-key.
    expect(mayBeShortcut(ev("F", "KeyF", 70, { shift: true }))).toBe(false);
    // With Ctrl or ⌘ any key may be one, as may Insert (a paste key).
    expect(mayBeShortcut(ev("c", "KeyC", 67, { ctrl: true }))).toBe(true);
    expect(mayBeShortcut(ev("v", "KeyV", 86, { meta: true }))).toBe(true);
    expect(mayBeShortcut(ev("Insert", "Insert", 45, { shift: true }))).toBe(true);
    expect(mayBeShortcut(ev("Insert", "Insert", 45))).toBe(true);
    // A browser's autofill sends a keydown with no key: no shortcut, no error.
    expect(mayBeShortcut({ ctrlKey: false, altKey: false, metaKey: false } as KeyboardEvent)).toBe(false);
  });

  it("returns early before the table is looked up, in the terminal and in the window", () => {
    const terminal = source("/src/components/TerminalView.tsx");
    const at = terminal.indexOf("term.attachCustomKeyEventHandler((ev) => {");
    expect(at).toBeGreaterThanOrEqual(0);
    const handler = terminal.slice(at);
    const gate = handler.indexOf("if (!Fork.mayBeShortcut(ev)) return true;");
    expect(gate, "the terminal's early return for plain typing is gone").toBeGreaterThanOrEqual(0);
    expect(gate).toBeLessThan(handler.indexOf("runShortcut(ev)"));
    // Nothing before it looks the key up or makes anything: the app's own keys
    // (behind their own Ctrl check) and the held-key repeats only.
    expect(handler.slice(0, gate)).not.toMatch(/comboOf|shortcutFor|matchShortcut|runShortcut/);
  });

  it("keeps Ctrl+Shift+W, Ctrl+Shift+K and Ctrl+Shift+Plus as the fork had them (#277)", () => {
    const keys = (a: Parameters<typeof defaultShortcuts>[0]) => defaultShortcuts(a).map(comboText);
    expect(keys("closeTerminal")).toContain("Ctrl+Shift+W");
    expect(keys("clear")).toContain("Ctrl+Shift+K");
    expect(keys("newTerminal")).toContain("Ctrl+Shift+Plus");
  });

  it("keeps the fork's Ctrl+Shift+J, Ctrl+Shift+L and Ctrl+Shift+P out of the table", () => {
    // Each is a key the table could take, so it has to be refused by name.
    for (const text of RESERVED_KEYS) {
      const c = parseCombo(text);
      expect(c && usableCombo(c), text).toBe(true);
      expect(comboText(c!)).toBe(text);
    }
    expect(RESERVED_KEYS).toEqual(expect.arrayContaining(["Ctrl+Shift+J", "Ctrl+Shift+L", "Ctrl+Shift+P", "Shift+Meta+P"]));
    // No action has one by default.
    for (const a of SHORTCUT_ACTIONS) {
      for (const k of defaultShortcuts(a).map(comboText)) expect(RESERVED_KEYS, `${a}: ${k}`).not.toContain(k);
    }
    // The table refuses them as it refuses the system's keys (ShortcutSettings).
    const settings = source("/src/components/ShortcutSettings.tsx");
    expect(settings).toContain("SYSTEM_KEYS.push(...RESERVED_KEYS);");
    expect(settings).toContain("if (SYSTEM_KEYS.includes(comboText(combo))) {");
    // And the keys still do what they did.
    expect(appHotkey({ code: "KeyJ", ctrlKey: true, shiftKey: true, altKey: false, metaKey: false } as KeyboardEvent)).toBe("jump");
  });
});

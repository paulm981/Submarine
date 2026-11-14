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

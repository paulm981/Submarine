// Checks that the fork's hooks in the original project's frontend files are
// still there: the page's half of src-tauri/src/fork_hooks.rs.
//
// The fork keeps its features in files of its own (FORK.md, "Keeping up with
// the original project"); the original project's files reach them through
// an import. A sync's merge can drop such a line and still build, when the
// code that used it went with it, and the feature is quietly gone. So each
// of the original project's files below lists the fork modules it imports,
// and this fails naming any import a merge lost, or a fork module that's
// gone. An import left in a comment doesn't count.
//
// Run it, with the page's other tests, from the repository root:
// npm --prefix tools/test ci once, then npm --prefix tools/test test.
//
// The table is every relative import, in a file that's in the original
// project (upstream/main), of a module that isn't. After a deliberate change
// (a hook added, moved or removed), update the table to match: add the new
// import under its file, or drop the one that went, in the same pull request.
//
// A file that reaches many of the fork's hooks through one import (DesktopApp,
// through src/fork/desktop) also lists them under `uses`, as written there:
// each must still be in the file's code, so a merge that drops one fails too.

import { describe, expect, it } from "vitest";

const HOOKS: { file: string; imports: string[]; uses?: string[] }[] = [
  {
    file: "src/DesktopApp.tsx",
    // Everything of the fork's in the main window comes through src/fork/desktop.
    imports: ["./fork/desktop"],
    uses: [
      "Fork.readForkSettings", "Fork.writeForkSettings", "Fork.useServerLooks", "Fork.useHighlightRules", "Fork.useHeldActions",
      "Fork.useLiveEditKept", "Fork.wouldEnd", "Fork.useOsBadges", "Fork.useCloseSessions", "Fork.useTabKeys", "Fork.useTabStrip",
      "Fork.useJumpHostPick", "Fork.useSessionTree", "Fork.aiPaneViewOf", "Fork.saveServerLooks", "Fork.sessionPaneStyle",
      "Fork.tabMenuStyle", "Fork.titleTabClass", "Fork.AppVersion", "Fork.HomeTab", "Fork.HomeView", "Fork.NewsDot",
      "Fork.TabAiMarker", "Fork.AllTabsButton", "Fork.AiTitleButtons", "Fork.TabMoveItems", "Fork.TabCloseItems",
      "Fork.DesktopSessionTree", "Fork.DesktopAiSide", "Fork.PingPanel", "Fork.DesktopAiPanel", "Fork.AiTrustModal",
    ],
  },
  {
    file: "src/components/AddNodePanel.tsx",
    imports: ["../ui/highlight", "../ui/terminalThemes", "../util/folderPath", "./JumpHostPicker", "./KeptSessions", "./TerminalSchemes"],
  },
  { file: "src/components/DockerTab.tsx", imports: ["../ui/rootInvoke", "../util/infoFont"] },
  { file: "src/components/FilePanel.tsx", imports: ["../util/fileName"] },
  { file: "src/components/InfoPanel.tsx", imports: ["../ui/rootInvoke", "../util/infoFont"] },
  { file: "src/components/NodeGrid.tsx", imports: ["../ui/ping", "../util/folderPath", "../util/jumpHostNote", "./OsBadge"] },
  { file: "src/components/ProfilePanel.tsx", imports: ["./StoreSync"] },
  { file: "src/components/ProfileSelectPage.tsx", imports: ["./StoreSync"] },
  {
    file: "src/components/SessionView.tsx",
    imports: ["../ui/aiPane", "../ui/tabDrag", "../ui/tabStyle", "../ui/terminalDock", "../util/ai", "./KeptSessions"],
  },
  { file: "src/components/SettingsPanel.tsx", imports: ["./settings"] },
  { file: "src/components/SftpWorkspace.tsx", imports: ["../util/fileName"] },
  {
    file: "src/components/TerminalView.tsx",
    imports: [
      "../ui/gpuSlots", "../ui/highlight", "../ui/rootInvoke", "../ui/terminalActivity", "../ui/terminalDock",
      "../ui/terminalThemes", "../util/clipboard", "../util/diagnostics",
    ],
  },
  { file: "src/main.tsx", imports: ["./util/diagnostics", "./util/nativeMenus", "./util/plainTyping"] },
  { file: "src/ui/confirm.tsx", imports: ["../util/fileName"] },
];

// Every source file's text, by its path from the repository root ("/src/…"):
// read by Vite, so the test needs no Node types under the app's tsconfig.
const SOURCES = import.meta.glob<string>("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true });

/** `src` without its comments: block ones, and those on a line of their own
 *  or after the code (" // …"). */
const codeOf = (src: string): string =>
  src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "").replace(/\s\/\/ .*$/gm, "");

/** The modules `src` imports (`import … from`, `import "…"`, `import("…")`,
 *  `export … from`), leaving out comments. */
const importsOf = (src: string): Set<string> =>
  new Set([...codeOf(src).matchAll(/\b(?:from|import)\s*\(?\s*["']([^"']+)["']/g)].map((m) => m[1]));

/** `name` is in the code (not only in a comment), as a whole word. */
const uses = (src: string, name: string): boolean =>
  new RegExp(`${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`).test(codeOf(src));

/** `spec` imported from `file`, as the file it names: "/src/…", or null. */
const resolve = (file: string, spec: string): string | null => {
  const parts = `/${file}`.split("/").slice(0, -1);
  for (const p of spec.split("/")) {
    if (p === "..") parts.pop();
    else if (p !== ".") parts.push(p);
  }
  const base = parts.join("/");
  return [".ts", ".tsx", "/index.ts", "/index.tsx"].map((e) => base + e).find((p) => p in SOURCES) ?? null;
};

describe("the fork's hooks in the original project's files", () => {
  for (const { file, imports, uses: used = [] } of HOOKS) {
    it(file, () => {
      const src = SOURCES[`/${file}`];
      expect(src, `${file} is gone: move its hooks to where its code went, and update this table`).toBeTypeOf("string");
      const found = importsOf(src);
      expect(imports.filter((i) => !found.has(i)), `fork imports ${file} no longer has`).toEqual([]);
      expect(imports.filter((i) => resolve(file, i) === null), `fork modules ${file} imports that are gone`).toEqual([]);
      expect(used.filter((u) => !uses(src, u)), `fork hooks ${file} no longer has`).toEqual([]);
    });
  }
});

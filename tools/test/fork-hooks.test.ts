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
// through src/fork/desktop; TerminalView, through src/fork/terminal;
// SessionView, through src/fork/session; AddNodePanel, through
// src/fork/server-form; NodeGrid, through src/fork/grid; ProfilePanel, through
// src/fork/profile) also lists them under `uses`, as written there:
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
      "Fork.DesktopSessionTree", "Fork.DesktopAiSide", "Fork.PingPanel", "Fork.DesktopAiPanel", "Fork.AiTrustModal", "Fork.TerminalTurn",
      "Fork.BulkTransfer", "Fork.BulkTabItems",
    ],
  },
  {
    file: "src/components/AddNodePanel.tsx",
    // Everything of the fork's in the server form comes through src/fork/server-form:
    // `Fork.…`, and what its hooks give (`forkImport.…`, jumpRowRef).
    imports: ["../fork/server-form"],
    uses: [
      "Fork.ImportedHostExtra", "Fork.useFocusJump", "Fork.protocolSwitch", "Fork.useImportResult", "Fork.parseClientImport",
      "Fork.rowKey", "Fork.importPicked", "Fork.folderParentId", "Fork.FolderOptions", "Fork.HeldPasswordNote", "Fork.JumpVia",
      "Fork.MoreFields", "Fork.ImportDone", "Fork.ChooseImportFile", "Fork.HostBadges", "Fork.HostDetails", "Fork.ImportSkipped",
      "Fork.ImportDoneButton", "forkImport.setSkipped", "forkImport.setDone", "forkImport.done", "forkImport.skipped",
      "jumpRowRef",
    ],
  },
  { file: "src/components/DockerTab.tsx", imports: ["../ui/rootInvoke", "../util/infoFont"] },
  { file: "src/components/FilePanel.tsx", imports: ["../util/fileName"] },
  { file: "src/components/HeldReview.tsx", imports: ["../util/ai"] },
  { file: "src/components/InfoPanel.tsx", imports: ["../ui/rootInvoke", "../util/infoFont"] },
  {
    file: "src/components/NodeGrid.tsx",
    // Everything of the fork's in the server grid comes through src/fork/grid:
    // `Fork.…`, and what its hooks give (`tree.…`, the fork's folders in the
    // original project's shape, and `cardMenu.…`).
    imports: ["../fork/grid"],
    uses: [
      "Fork.useFolderTree", "Fork.useCardMenu", "Fork.OsAvatar", "Fork.jumpHostNote", "Fork.deleteFolderMessage",
      "Fork.FolderPath", "Fork.CardMenu", "tree.serverCount", "cardMenu.open", "cardMenu.close",
    ],
  },
  {
    file: "src/components/ProfilePanel.tsx",
    // Everything of the fork's in the Profile panel comes through src/fork/profile:
    // `Fork.…`, and what its hook gives (`ownStore.…`).
    imports: ["../fork/profile"],
    uses: ["Fork.StoreStats", "Fork.useStoreSync", "Fork.StoreSection", "ownStore.read", "ownStore.store", "ownStore.cloudOn"],
  },
  { file: "src/components/ProfileSelectPage.tsx", imports: ["./StoreSync"] },
  {
    file: "src/components/SessionView.tsx",
    // Everything of the fork's in a server's tab comes through src/fork/session:
    // `Fork.…`, and what its hooks give (`jumpGone.…`, `split.…`, onShellReadyFor).
    imports: ["../fork/session"],
    uses: [
      "Fork.useJumpGone", "Fork.useTermDrag", "Fork.useShareWithAi", "Fork.useOnConnect", "Fork.diagnoseConnect",
      "Fork.splitShown", "Fork.splitActions", "Fork.stripLook", "Fork.termTabProps", "Fork.termTabLook",
      "Fork.termCloseLook", "Fork.paneLook", "Fork.TermTabLabel", "Fork.SplitButton", "Fork.ShareWithAiButton",
      "Fork.SplitBesideItems", "Fork.AiBesideItems", "Fork.KeptSessions", "Fork.TermMoveItems", "Fork.ShareWithAiMenu",
      "Fork.JumpGoneChoices", "jumpGone.refused", "jumpGone.clear", "split.closeTerminal", "split.splitWithNew",
      "split.splitWith", "split.removeFromSplit", "onShellReadyFor", "Fork.TerminalTurn",
    ],
  },
  { file: "src/components/SettingsPanel.tsx", imports: ["./settings"] },
  { file: "src/components/SftpWorkspace.tsx", imports: ["../util/fileName"] },
  {
    file: "src/components/TerminalView.tsx",
    // Everything of the fork's in a terminal comes through src/fork/terminal:
    // `Fork.…`, the state it keeps (`fork.…`) and its part of the xterm
    // (`forkTerm.…`).
    imports: ["../fork/terminal"],
    uses: [
      "Fork.TerminalProps", "Fork.useTerminal", "Fork.rootOpened", "Fork.schemeForServer", "Fork.savedPasswords",
      "Fork.xtermLook", "Fork.mountTerminal", "Fork.readClipboardText", "Fork.appHotkey", "Fork.pasteClean",
      "Fork.TerminalOverlay", "fork.commands.write", "fork.commands.resize", "fork.commands.close", "fork.resetHighlight",
      "fork.outputChannel", "fork.rootInvoke", "fork.watchForShellReady", "fork.dockElRef", "fork.dockEl", "fork.onScreen",
      "forkTerm.startGpu", "forkTerm.openLocal", "forkTerm.output", "forkTerm.write", "forkTerm.applyLook",
      "forkTerm.register", "forkTerm.unmount", "forkTerm.dispose", "forkTerm.undock",
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

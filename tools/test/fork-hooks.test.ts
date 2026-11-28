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
      "Fork.useLiveEditKept", "Fork.useBroadcastWall", "Fork.broadcastSwitch", "Fork.useWouldEnd", "Fork.useOsBadges", "Fork.useCloseSessions", "Fork.useTabKeys", "Fork.useTabStrip",
      "Fork.useJumpHostPick", "Fork.useSessionTree", "Fork.aiPaneViewOf", "Fork.saveServerLooks", "Fork.sessionPaneStyle",
      "Fork.tabMenuStyle", "Fork.tabDivider", "Fork.AppVersion", "Fork.HomeTab", "Fork.HomeView", "Fork.NewsDot",
      "Fork.TabAiMarker", "Fork.AllTabsButton", "Fork.AiTitleButtons", "Fork.TabMoveItems", "Fork.TabCloseItems",
      "Fork.DesktopSessionTree", "Fork.DesktopAiSide", "Fork.PingPanel", "Fork.DesktopAiPanel", "Fork.AiTrustModal", "Fork.TerminalTurn",
      "Fork.BulkTransfer", "Fork.BulkTabItems", "Fork.BroadcastBar", "Fork.BroadcastTileChips", "Fork.BroadcastTab", "Fork.shownTabs",
      // The original project's shortcuts table (#374): the early return for plain
      // typing, and Next tab and Previous tab in the fork's order.
      "if (!Fork.mayBeShortcut(e)) return", "Fork.tabStepList(orderedSessionsRef.current",
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
  { file: "src/components/Feedback.tsx", imports: ["../fork/feedback"], uses: ["if (feedbackOnGithub"] },
  { file: "src/components/FilePanel.tsx", imports: ["../util/fileName", "../fork/terminal/here"], uses: ["TerminalHereItem"] },
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
      "Fork.JumpGoneChoices", "jumpGone.refused", "jumpGone.clear", "Fork.closeTabAsking", "split.splitWithNew",
      "split.splitWith", "split.removeFromSplit", "onShellReadyFor", "Fork.TerminalTurn", "Fork.TerminalHereListener",
    ],
  },
  { file: "src/components/SettingsPanel.tsx", imports: ["./settings"] },
  // The fork's keys beside the shortcuts table (#374): refused there, and named under it.
  { file: "src/components/ShortcutSettings.tsx", imports: ["../fork/keys"], uses: ["SYSTEM_KEYS.push(...RESERVED_KEYS", "<ReservedKeysNote"] },
  { file: "src/components/SftpWorkspace.tsx", imports: ["../util/fileName"] },
  {
    file: "src/components/TerminalView.tsx",
    // Everything of the fork's in a terminal comes through src/fork/terminal:
    // `Fork.…`, the state it keeps (`fork.…`) and its part of the xterm
    // (`forkTerm.…`).
    imports: ["../fork/terminal"],
    uses: [
      "Fork.TerminalProps", "Fork.broadcastWrite(terminalId", "Fork.fileMenuAt", "Fork.useTerminal", "Fork.rootOpened", "Fork.schemeForServer", "Fork.savedPasswords",
      "Fork.xtermLook", "Fork.mountTerminal", "Fork.appHotkey", "if (!Fork.mayBeShortcut(ev)) return true", "Fork.pasteClean",
      "Fork.TerminalOverlay", "fork.write", "fork.commands.resize", "fork.commands.close", "fork.resetHighlight",
      "fork.outputChannel", "fork.rootInvoke", "fork.watchForShellReady", "fork.dockEl", "fork.onScreen",
      "forkTerm.startGpu", "forkTerm.openLocal", "forkTerm.output", "forkTerm.write", "forkTerm.applyLook",
      "forkTerm.register", "forkTerm.unmount", "forkTerm.dispose", "forkTerm.undock",
    ],
  },
  // Report this reply goes to the fork's GitHub, with the fork's words.
  { file: "src/components/ai/ChatPanel.tsx", imports: ["../../fork/feedback"], uses: ["...REPORT_REPLY_CONFIRM"] },
  { file: "src/components/CmdsPanel.tsx", imports: ["../fork/secretCmdReview"], uses: ["secretReviewOf"] },
  { file: "src/main.tsx", imports: ["./util/diagnostics", "./util/nativeMenus", "./util/plainTyping", "./fork/dialogFocus.css"] },
  { file: "src/ui/confirm.tsx", imports: ["../util/fileName", "../fork/secretCmdReview"], uses: ["isConfirmOkLocked", "disabled={confirmLocked"] },
  { file: "src/util/pasteText.ts", imports: ["../fork/secretCmdReview"], uses: ["secretConfirmExtras"] },
];

// Every source file's text, by its path from the repository root ("/src/…"):
// read by Vite, so the test needs no Node types under the app's tsconfig.
const SOURCES = import.meta.glob<string>("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true });
// Style files a hook imports (src/fork/dialogFocus.css): found by name, not read.
const STYLES = new Set(Object.keys(import.meta.glob("/src/**/*.css")));

/** `src` without its comments: block ones, and those on a line of their own
 *  or after the code (" // …"). A string is kept as it is, so a comment's
 *  marks inside one ("db/*.sql", "https://…") don't start a comment. A
 *  quoted string ends at its line's end, so an apostrophe in JSX text
 *  (`Don't`) holds at most the rest of that line; a template literal runs
 *  to its closing backtick, with the code in its `${…}` read as code. */
const codeOf = (src: string): string => {
  let out = "";
  let i = 0;
  let lineHasCode = false;
  // For each template literal whose `${…}` is open: the brace depth at its `${`.
  const open: number[] = [];
  let depth = 0;
  // A template literal's text from `i`: to its closing backtick, or into a `${`.
  const template = () => {
    while (i < src.length) {
      const c = src[i];
      if (c === "\\") { out += src.slice(i, i + 2); i += 2; continue; }
      if (c === "`") { out += c; i++; return; }
      if (c === "$" && src[i + 1] === "{") { out += "${"; i += 2; open.push(depth); depth++; return; }
      out += c;
      i++;
    }
  };
  while (i < src.length) {
    const c = src[i];
    const next = src[i + 1];
    if (c === "/" && next === "*") {
      const end = src.indexOf("*/", i + 2);
      i = end < 0 ? src.length : end + 2;
      continue;
    }
    if (c === "/" && next === "/" && (!lineHasCode || (/\s/.test(src[i - 1] ?? "") && src[i + 2] === " "))) {
      const end = src.indexOf("\n", i);
      i = end < 0 ? src.length : end;
      continue;
    }
    if (c === "\n") lineHasCode = false;
    else if (!/\s/.test(c)) lineHasCode = true;
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < src.length && src[j] !== c && src[j] !== "\n") j += src[j] === "\\" ? 2 : 1;
      if (src[j] === c) j++;
      out += src.slice(i, j);
      i = j;
      continue;
    }
    if (c === "`") { out += c; i++; template(); continue; }
    if (c === "{") depth++;
    if (c === "}") {
      depth--;
      if (open.length > 0 && open[open.length - 1] === depth) {
        open.pop();
        out += c;
        i++;
        template();
        continue;
      }
    }
    out += c;
    i++;
  }
  return out;
};

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
  if (base.endsWith(".css")) return STYLES.has(base) ? base : null;
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

describe("codeOf, the comment stripper", () => {
  it("keeps a string with a comment's marks in it, and the code after it", () => {
    // The original project's mirror help text (#416) has "db/*.sql" in it.
    const src = 'const help = t("With a / it starts at the folder (db/*.sql).");\nconst jumpRowRef = Fork.useFocusJump(isOpen);\nconst u = "https://example.com"; // a comment\n';
    const code = codeOf(src);
    expect(code).toContain('"With a / it starts at the folder (db/*.sql)."');
    expect(code).toContain("Fork.useFocusJump(isOpen)");
    expect(code).toContain('"https://example.com"');
    expect(code).not.toContain("a comment");
  });
  it("still drops comments: block, on a line of their own, and after the code", () => {
    const src = "/* Fork.gone(1) */\n  // Fork.gone(2)\nkeep(); // Fork.gone(3)\n/*\n Fork.gone(4)\n*/ after();\n";
    const code = codeOf(src);
    expect(code).not.toContain("Fork.gone");
    expect(code).toContain("keep();");
    expect(code).toContain("after();");
  });
  it("reads a template literal's ${…} as code, and its text as text", () => {
    const src = "const a = `/* text ${Fork.kept(x) /* gone */} more */`; Fork.after();\nconst b = `x ${ { y: 1 }.y } // text`;\n";
    const code = codeOf(src);
    expect(code).toContain("`/* text ${Fork.kept(x) } more */`");
    expect(code).not.toContain("gone");
    expect(code).toContain("Fork.after();");
    expect(code).toContain("// text`");
  });
  it("lets an apostrophe in JSX text hold only the rest of its line", () => {
    const src = "<p>Don't /* a */ do it</p>\n/* Fork.gone() */ Fork.kept();\n";
    const code = codeOf(src);
    expect(code).not.toContain("Fork.gone");
    expect(code).toContain("Fork.kept();");
  });
});

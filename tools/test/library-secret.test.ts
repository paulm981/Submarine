// The Library's Secret commands (their #356, the fork's #283): where a
// secret's text goes, and where it never goes.
//
// A secret is a mask on screen. Its text is kept and synced like any
// command's (in `commands.content`), so these checks pin what the fork
// promises about it, and fail when a change or a sync breaks one:
// - a run always asks first, with the text hidden until Show command, and
//   goes the fork's paste way (control characters out, one terminal);
// - it's masked wherever the page shows Library text;
// - no AI tool can read the Library, and a run isn't logged or kept in
//   command history.

import { beforeAll, describe, expect, it, vi } from "vitest";
import type { ConfirmOptions } from "../../src/ui/confirm";
import type { TerminalPaste } from "../../src/util/pasteText";

const raw = (files: Record<string, string>) => Object.entries(files);
const page = raw(import.meta.glob<string>(["/src/**/*.ts", "/src/**/*.tsx"], { query: "?raw", import: "default", eager: true }));
const one = (path: string) => {
  const hit = page.find(([p]) => p === path);
  if (!hit) throw new Error(`${path} is gone`);
  return hit[1];
};
// Every backend file the AI reaches: the fork's AI tools (the MCP server and
// its helper), the original project's assistant and its agents' gateway.
const aiRust = raw(import.meta.glob<string>(
  ["/src-tauri/src/ai_tabs/**/*.rs", "/src-tauri/src/ai_tabs.rs", "/src-tauri/src/ai/**/*.rs", "/src-tauri/src/ai.rs",
   "/src-tauri/src/gateway/**/*.rs", "/src-tauri/src/gateway.rs", "/src-tauri/src/agent_auth.rs"],
  { query: "?raw", import: "default", eager: true },
));
const libRs = (import.meta.glob<string>("/src-tauri/src/lib.rs", { query: "?raw", import: "default", eager: true }))["/src-tauri/src/lib.rs"];

// Code without its comments, so a mention in a comment doesn't count.
const code = (text: string) => text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

describe("running a secret", () => {
  const sent: TerminalPaste[] = [];
  let writeToTerminal: typeof import("../../src/util/pasteText").writeToTerminal;
  beforeAll(async () => {
    const target = new EventTarget();
    vi.stubGlobal("window", target);
    const mod = await import("../../src/util/pasteText");
    writeToTerminal = mod.writeToTerminal;
    target.addEventListener(mod.TERMINAL_PASTE, (e) => sent.push((e as CustomEvent<TerminalPaste>).detail));
  });

  it("always asks, even for one line, with the text hidden until Show command", async () => {
    sent.length = 0;
    const asked: ConfirmOptions[] = [];
    const ok = await writeToTerminal({ terminalId: "t1", text: "mysql -p'hunter2'", enter: true, secret: true, confirm: async (o) => { asked.push(o); return true; } });
    expect(ok).toBe(true);
    expect(asked).toHaveLength(1);
    expect(asked[0].message).not.toContain("hunter2");
    expect(asked[0].revealableText).toBe("mysql -p'hunter2'");
    // Cancel has the focus: an Enter meant for the terminal can't run it.
    expect(asked[0].safeDefault).toBe(true);
    expect(sent).toEqual([{ terminalId: "t1", text: "mysql -p'hunter2'", enter: true }]);
  });

  it("sends nothing when the box is cancelled", async () => {
    sent.length = 0;
    expect(await writeToTerminal({ terminalId: "t1", text: "secret", enter: true, secret: true, confirm: async () => false })).toBe(false);
    expect(sent).toEqual([]);
  });

  it("takes hidden characters out before it asks, so what Show command shows is what runs", async () => {
    sent.length = 0;
    const asked: ConfirmOptions[] = [];
    await writeToTerminal({ terminalId: "t1", text: "echo ok​‮", enter: true, secret: true, confirm: async (o) => { asked.push(o); return true; } });
    expect(asked[0].revealableText).toBe("echo ok");
    expect(sent[0].text).toBe("echo ok");
  });

  it("goes the fork's paste way: control characters out, kept out of history, to its own terminal only", () => {
    const view = code(one("/src/components/TerminalView.tsx"));
    // The Library's text reaches xterm through pasteClean (withoutPasteControls).
    expect(view).toContain("Fork.pasteClean(term, text, pastingRef)");
    // The paste request is written to its terminal and returns before
    // Broadcast and before command history.
    const request = view.indexOf("const request = pasteRequestRef.current;");
    expect(request).toBeGreaterThan(0);
    const block = view.slice(request, view.indexOf("}", view.indexOf("return;", request)) + 1);
    expect(block).toContain("writeTerminal(terminalId,");
    expect(block).toContain("return;");
    expect(block).not.toContain("broadcastInput");
    expect(view.indexOf("cmd_history_add")).toBeGreaterThan(request);
    expect(view.indexOf("if (!TERMINAL_REPLY.test(data)) broadcastInput(bytes);")).toBeGreaterThan(request);
  });

  it("hides the text in the box until asked, and hides it again for the next box", () => {
    const box = code(one("/src/ui/confirm.tsx"));
    expect(box).toContain("confirmRevealed && state.opts.revealableText ? state.opts.revealableText : state.opts.message");
    const prime = box.slice(box.indexOf("const primeText"), box.indexOf("}, []);", box.indexOf("const primeText")));
    expect(prime).toContain("setConfirmRevealed(false)");
    // Enter on the Show command toggle flips it and never answers the box.
    expect(box).toContain("isPanelControl(e.target, confirmPanelRef.current)");
  });
});

describe("a secret on screen", () => {
  it("is dots on the Library's cards and the session panel's, with no Paste and no tooltip of its text", () => {
    const library = code(one("/src/DesktopApp.tsx"));
    expect(library).toContain('{cmd.is_secret ? "••••••" : cmd.content}');
    const panel = code(one("/src/components/CmdsPanel.tsx"));
    expect(panel).toContain('{c.is_secret ? "••••••" :');
    expect(panel).toContain('title={c.is_secret ? t("Secret command — its content is hidden.") : c.content}');
    expect(panel).toMatch(/\{!c\.is_secret && \(\s*<button\s+onClick=\{\(\) => toTerminal\(c\.content, false\)\}/);
    expect(panel).toContain("toTerminal(c.content, true, cutOnCard(e.currentTarget), !!c.is_secret)");
  });

  it("isn't shown by any other part of the page: only the Library and the session panel load commands", () => {
    const loaders = page.filter(([, text]) => /invoke(<[^>]*>)?\(\s*["']get_commands["']/.test(code(text))).map(([p]) => p).sort();
    expect(loaders).toEqual(["/src/DesktopApp.tsx", "/src/components/CmdsPanel.tsx"]);
    // The session panel is a server tab's alone; Broadcast's Send to all,
    // the AI panes and the rest of the fork's page never reach it.
    const panelUsers = page.filter(([p, text]) => p !== "/src/components/CmdsPanel.tsx" && /from ["'][./]*\/?(components\/)?CmdsPanel["']/.test(text)).map(([p]) => p);
    expect(panelUsers).toEqual(["/src/components/SessionView.tsx"]);
    for (const [p, text] of page.filter(([p]) => p.startsWith("/src/fork/") || /\/Ai[A-Z]|\/ai\//.test(p))) {
      expect(code(text), p).not.toMatch(/get_commands|CmdsPanel|is_secret/);
    }
  });
});

describe("what can never see a secret", () => {
  it("no AI tool reads the Library: no backend file the AI reaches touches the commands table", () => {
    expect(aiRust.length).toBeGreaterThan(20);
    for (const [p, text] of aiRust) {
      expect(code(text), p).not.toMatch(/\b(FROM|JOIN|INTO|UPDATE|TABLE)\s+commands\b|get_commands|is_secret/i);
    }
  });

  it("the backend doesn't log a command's text when it's saved, changed or listed", () => {
    const start = libRs.indexOf("async fn add_command(");
    const end = libRs.indexOf("cmd_history_add", libRs.indexOf("async fn get_commands("));
    expect(start).toBeGreaterThan(0);
    expect(end).toBeGreaterThan(start);
    const body = code(libRs.slice(start, end));
    expect(body).not.toMatch(/println!|eprintln!|dbg!|log::|tracing::|applog|app_log|\.emit\(/);
  });

  it("the page doesn't log it either", () => {
    for (const p of ["/src/components/CmdsPanel.tsx", "/src/util/pasteText.ts", "/src/ui/confirm.tsx", "/src/components/SecretTextarea.tsx", "/src/util/commandTags.ts"]) {
      const text = code(one(p));
      // CmdsPanel's two console.error calls log the error, never a command.
      expect(text.match(/console\.\w+\([^)]*\)/g) ?? [], p).toEqual(
        p === "/src/components/CmdsPanel.tsx" ? ['console.error("Failed to fetch Library data:", e)', 'console.error("Clipboard write failed:", e)'] : [],
      );
      expect(text, p).not.toMatch(/addLog|app_log|cmd_history_add|ai_audit/);
    }
  });
});

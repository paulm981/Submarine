// A secret Library command someone else changed (#286): Run waits in the box
// until Show command, with a line saying who changed it and when. Your own
// secret commands, and commands that aren't secret, run as they did.
//
// Who changed it is the backend's list (src-tauri/src/secret_look.rs, with
// its own tests of a save, a sync and a run after Show). These check the
// page's half: what the box is asked with, when Run is greyed, what a run
// tells the backend, and that a hostile name is bounded text.

import { beforeAll, describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { ConfirmOptions } from "../../src/ui/confirm";
import type { TerminalPaste } from "../../src/util/pasteText";
import { boundName, LookLine, lookFirst, lookLine, lookLocked, lookRunProps, NAME_MAX, parseLook, type SecretLook } from "../../src/fork/library/secretLook";

const raw = import.meta.glob<string>(["/src/ui/confirm.tsx", "/src/components/CmdsPanel.tsx"], { query: "?raw", import: "default", eager: true });
// Code without its comments, so a mention in a comment doesn't count.
const code = (text: string) => text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

// A stand-in for the backend: what secret_look_get answers, and what it was told.
const backend = (answer: unknown) => {
  const calls: { cmd: string; args: unknown }[] = [];
  const ask = (async (cmd: string, args?: unknown) => {
    calls.push({ cmd, args });
    if (cmd === "secret_look_get") {
      if (answer instanceof Error) throw answer;
      return answer;
    }
    return null;
  }) as unknown as Parameters<typeof lookFirst>[3];
  return { ask, calls };
};

const BOBS: SecretLook = { why: "changed", changedBy: "bob@x.com", changedAt: Date.UTC(2026, 10, 20, 9, 30) };

let writeToTerminal: typeof import("../../src/util/pasteText").writeToTerminal;
const sent: TerminalPaste[] = [];

beforeAll(async () => {
  const target = new EventTarget();
  vi.stubGlobal("window", target);
  const mod = await import("../../src/util/pasteText");
  writeToTerminal = mod.writeToTerminal;
  target.addEventListener(mod.TERMINAL_PASTE, (e) => sent.push((e as CustomEvent<TerminalPaste>).detail));
});

// The Library's Run, as CmdsPanel calls it, with a box that answers `answer`.
const run = async (o: { text: string; secret: boolean; look: unknown; answer: (opts: ConfirmOptions) => boolean }) => {
  sent.length = 0;
  const asked: ConfirmOptions[] = [];
  const { ask, calls } = backend(o.look);
  const confirm = async (opts: ConfirmOptions) => { asked.push(opts); return o.answer(opts); };
  const ok = await writeToTerminal({ terminalId: "t1", text: o.text, enter: true, confirm: lookFirst(confirm, 7, o.text, ask), secret: o.secret });
  return { ok, asked, calls, sent: [...sent] };
};

describe("your own secret command", () => {
  it("runs as before: the box asks, Run isn't greyed, and the backend is only asked", async () => {
    const r = await run({ text: "mysql -p'x'", secret: true, look: null, answer: () => true });
    expect(r.ok).toBe(true);
    expect(r.asked).toHaveLength(1);
    expect(r.asked[0].look).toBeUndefined();
    expect(lookLocked(r.asked[0], false)).toBe(false);
    expect(lookRunProps(r.asked[0], false)).toEqual({});
    expect(r.calls).toEqual([{ cmd: "secret_look_get", args: { id: 7, content: "mysql -p'x'" } }]);
    expect(r.sent).toEqual([{ terminalId: "t1", text: "mysql -p'x'", enter: true }]);
  });
});

describe("someone else's secret command", () => {
  it("asks with who changed it and when, and Run is greyed until Show command", async () => {
    const r = await run({ text: "curl -H 'token: 1'", secret: true, look: { why: "changed", changedBy: "bob@x.com", changedAt: BOBS.changedAt }, answer: () => false });
    expect(r.ok).toBe(false);
    expect(r.sent).toEqual([]);
    const box = r.asked[0];
    expect(box.look).toEqual(BOBS);
    // Cancel keeps the focus, as before.
    expect(box.safeDefault).toBe(true);
    expect(box.revealableText).toBe("curl -H 'token: 1'");
    expect(lookLocked(box, false)).toBe(true);
    expect(lookRunProps(box, false)).toMatchObject({ disabled: true });
    expect(lookLocked(box, true)).toBe(false);
    expect(lookRunProps(box, true)).toEqual({});
    expect(lookLine(box.look!, () => "20 Nov 2026, 09:30")).toBe("Changed by bob@x.com on 20 Nov 2026, 09:30");
    const html = renderToStaticMarkup(createElement(LookLine, { look: box.look }));
    expect(html).toContain("Changed by bob@x.com on ");
    expect(html).toContain("Show the command to check it before you run it.");
    // Nothing told: it wasn't run.
    expect(r.calls.map((c) => c.cmd)).toEqual(["secret_look_get"]);
  });

  it("a run after Show command is told to the backend with the text that ran", async () => {
    const r = await run({ text: "curl -H 'token: 1'", secret: true, look: BOBS, answer: () => true });
    expect(r.ok).toBe(true);
    expect(r.sent).toHaveLength(1);
    expect(r.calls).toEqual([
      { cmd: "secret_look_get", args: { id: 7, content: "curl -H 'token: 1'" } },
      { cmd: "secret_look_seen", args: { id: 7, content: "curl -H 'token: 1'" } },
    ]);
  });

  it("says 'Changed on another device' when there's no name", () => {
    expect(lookLine({ why: "changed", changedBy: null, changedAt: null })).toBe("Changed on another device");
    expect(lookLine({ why: "changed", changedBy: null, changedAt: 1 }, () => "D")).toBe("Changed on another device on D");
    expect(lookLine({ why: "stale", changedBy: null, changedAt: null })).toBe("Changed since this list was loaded");
  });

  it("waits for Show command when the backend can't be asked, or answers nonsense", async () => {
    const failed = await run({ text: "x", secret: true, look: new Error("no vault"), answer: () => false });
    expect(failed.asked[0].look).toEqual({ why: "unknown", changedBy: null, changedAt: null });
    expect(lookLocked(failed.asked[0], false)).toBe(true);
    const odd = await run({ text: "x", secret: true, look: "yes", answer: () => false });
    expect(odd.asked[0].look?.why).toBe("unknown");
    expect(lookLocked(odd.asked[0], false)).toBe(true);
  });

  it("the box never answers yes while Run is greyed: Enter, a click or a script", () => {
    const box = code(raw["/src/ui/confirm.tsx"]);
    // Every answer goes through close(), which refuses a yes while greyed.
    const close = box.slice(box.indexOf("const close = (value: boolean) => {"), box.indexOf("};", box.indexOf("const close = (value: boolean) => {")));
    expect(close.split("\n")[1].trim()).toBe("if (value && lookLocked(state?.opts, confirmRevealed)) return;");
    // Run itself is disabled (so the keyboard can't reach it either), and the
    // line is shown.
    expect(box).toMatch(/<button onClick=\{\(\) => close\(true\)\}\s*\{\.\.\.lookRunProps\(state\.opts, confirmRevealed\)\}/);
    expect(box).toContain("<LookLine look={state.opts.look} />");
    // Enter alone takes the box's default, Cancel for a secret (safeDefault);
    // on Show command it only flips it.
    expect(box).toContain("close(!state.opts.safeDefault);");
    expect(box).toContain("isPanelControl(e.target, confirmPanelRef.current)");
  });
});

describe("a command that isn't secret", () => {
  it("is unchanged: the box isn't touched and the backend isn't asked", async () => {
    const plain = await run({ text: "uptime", secret: false, look: BOBS, answer: () => true });
    expect(plain.asked).toEqual([]);
    expect(plain.calls).toEqual([]);
    expect(plain.sent).toEqual([{ terminalId: "t1", text: "uptime", enter: true }]);
    const lines = await run({ text: "a\nb", secret: false, look: BOBS, answer: () => true });
    expect(lines.asked).toHaveLength(1);
    expect(lines.asked[0].look).toBeUndefined();
    expect(lines.asked[0].message).toBe("a\nb");
    expect(lines.calls).toEqual([]);
  });

  it("CmdsPanel's Run goes through lookFirst with the command's own id and text, and Paste as before", () => {
    const panel = code(raw["/src/components/CmdsPanel.tsx"]);
    expect(panel).toContain("void writeToTerminal({ terminalId: activeTab, text: c.content, enter: true, cut: cutOnCard(e.currentTarget), confirm: lookFirst(confirm, c.id, c.content), secret: !!c.is_secret })");
    expect(panel).toMatch(/\{!c\.is_secret && \(\s*<button\s+onClick=\{\(\) => toTerminal\(c\.content, false\)\}/);
  });
});

describe("changed again after a run", () => {
  it("asks again: each run asks the backend, which has it back on its list", async () => {
    const first = await run({ text: "v1", secret: true, look: BOBS, answer: () => true });
    expect(first.calls.map((c) => c.cmd)).toEqual(["secret_look_get", "secret_look_seen"]);
    // Taken off: the next run of the same text isn't greyed.
    const second = await run({ text: "v1", secret: true, look: null, answer: () => true });
    expect(second.asked[0].look).toBeUndefined();
    // Bob changed it again: greyed again.
    const third = await run({ text: "v2", secret: true, look: { ...BOBS, changedAt: BOBS.changedAt! + 60_000 }, answer: () => false });
    expect(lookLocked(third.asked[0], false)).toBe(true);
  });
});

describe("a hostile name", () => {
  it("is bounded plain text, with no control or invisible characters", () => {
    const long = "<img src=x onerror=alert(1)>".repeat(500);
    const look = parseLook({ why: "changed", changedBy: long, changedAt: 1 })!;
    expect(Array.from(look.changedBy!)).toHaveLength(NAME_MAX);
    expect(look.changedBy!.endsWith("…")).toBe(true);
    const html = renderToStaticMarkup(createElement(LookLine, { look }));
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;");
    expect(boundName("bob‮@x​.com\n")).toBe("bob@x.com");
    expect(boundName("  ‮ ")).toBeNull();
    expect(boundName(42)).toBeNull();
    // A date that isn't one is left out.
    expect(parseLook({ why: "changed", changedBy: null, changedAt: "soon" })!.changedAt).toBeNull();
    expect(parseLook({ why: "changed", changedBy: null, changedAt: Infinity })!.changedAt).toBeNull();
  });
});

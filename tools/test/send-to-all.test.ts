// Send to all (#218): a set of commands typed into every Broadcast server.

import { beforeEach, describe, expect, it, vi } from "vitest";

const calls: { ids: string[]; data: number[] }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: { terminalIds?: string[]; terminalId?: string; data: number[] }) => {
    calls.push({ ids: cmd === "write_terminals_data" ? args.terminalIds! : [args.terminalId!], data: args.data });
    return null;
  },
}));

const { lineCount, MAX_SEND, prepareSend } = await import("../../src/fork/terminal/sendToAll");
const { aimNow, resetAim, sendTargets, setOrder, setPaused, typeToOne } = await import("../../src/fork/terminal/broadcastAim");
const { broadcastSend, broadcastWrite, sent } = await import("../../src/fork/terminal/broadcast");
const { leftOutWords } = await import("../../src/fork/desktop/sendToAllDialog");

const text = (data: number[]) => new TextDecoder().decode(new Uint8Array(data));
const ALL = ["a", "b", "c"];

beforeEach(() => {
  resetAim();
  setOrder(ALL);
  calls.length = 0;
});

describe("what Send types", () => {
  it("types each line with an Enter, and one after the last line when asked", () => {
    const p = prepareSend("cd /tmp\nls", true);
    expect(p.ok && text(p.data)).toBe("cd /tmp\rls\r");
    expect(p.ok && p.lines).toBe(2);
  });

  it("leaves the last line unrun when Enter after it isn't asked for", () => {
    const p = prepareSend("cd /tmp\nls", false);
    expect(p.ok && text(p.data)).toBe("cd /tmp\rls");
  });

  it("doesn't add a second Enter after a last line that has one", () => {
    const p = prepareSend("ls\n", true);
    expect(p.ok && text(p.data)).toBe("ls\r");
    expect(p.ok && p.lines).toBe(1);
  });

  it("takes Windows line breaks as one Enter each", () => {
    const p = prepareSend("a\r\nb\r\n", true);
    expect(p.ok && text(p.data)).toBe("a\rb\r");
  });

  it("lets no escape sequence or control key through, and says so", () => {
    const p = prepareSend("echo hi\x1b[201~\x03; rm x\x04", true);
    expect(p.ok && text(p.data)).toBe("echo hi[201~; rm x\r");
    expect(p.ok && p.cleaned).toBe(true);
  });

  it("refuses nothing, only control characters, or too much", () => {
    expect(prepareSend("  \n ", true)).toEqual({ ok: false, why: "Nothing to send." });
    expect(prepareSend("\x03\x04", true)).toEqual({ ok: false, why: "Nothing to send: it was only control characters." });
    expect(prepareSend("x".repeat(MAX_SEND + 1), true).ok).toBe(false);
    expect(prepareSend("x".repeat(MAX_SEND), true).ok).toBe(true);
  });

  it("counts lines as they'll be sent", () => {
    expect(lineCount("")).toBe(0);
    expect(lineCount("a")).toBe(1);
    expect(lineCount("a\n")).toBe(1);
    expect(lineCount("a\r\nb")).toBe(2);
  });
});

describe("who Send reaches", () => {
  it("everyone taking what's typed: not the paused, not those waiting for the next line", async () => {
    setPaused(["c"], true);
    expect(sendTargets(aimNow(), ALL)).toEqual({ to: ["a", "b"], paused: 1, waiting: 0 });
    // A line half typed to a and b; c resumed now waits for the next.
    void broadcastWrite("a", { terminalId: "b", data: [0x6c] });
    await sent();
    setPaused(["c"], false);
    expect(sendTargets(aimNow(), ALL)).toEqual({ to: ["a", "b"], paused: 0, waiting: 1 });
  });

  it("goes to all it's given in one call, while typing to one too", async () => {
    typeToOne("a");
    const p = prepareSend("uptime", true);
    if (!p.ok) throw new Error(p.why);
    broadcastSend(ALL, p.data);
    await sent();
    expect(calls).toEqual([{ ids: ALL, data: p.data }]);
  });

  it("ends the line with the servers it went to; one it skipped joins at the next line", async () => {
    const half = prepareSend("tar czf x.tgz", false);
    if (!half.ok) throw new Error(half.why);
    broadcastSend(["a", "b"], half.data);
    await sent();
    expect(aimNow().line).toEqual(new Set(["a", "b"]));
    expect(sendTargets(aimNow(), ALL).to).toEqual(["a", "b"]);
    const end = prepareSend(" /etc", true);
    if (!end.ok) throw new Error(end.why);
    broadcastSend(["a", "b"], end.data);
    await sent();
    expect(aimNow().line).toBeNull();
    expect(sendTargets(aimNow(), ALL).to).toEqual(ALL);
  });

  it("sends nothing to nobody", async () => {
    broadcastSend([], [0x61]);
    await sent();
    expect(calls).toEqual([]);
  });

  it("says who's left out in words", () => {
    expect(leftOutWords({ paused: 1, waiting: 0, offline: 2 })).toBe("1 paused, 2 not connected");
    expect(leftOutWords({ paused: 0, waiting: 3, offline: 0 })).toBe("3 waiting for the next line");
    expect(leftOutWords({ paused: 0, waiting: 0, offline: 0 })).toBe("");
  });
});

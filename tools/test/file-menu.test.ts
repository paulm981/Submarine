// The right-click menu on a file name (#40, #202) and "Open terminal here"'s
// `cd` (#72, #199), from an independent review of their tests: the word under
// the pointer read from a real xterm's cells, the host a path is on, and
// the `cd` after the on-connect commands.

import { Terminal } from "@xterm/xterm";
import { beforeEach, describe, expect, it, vi } from "vitest";

const writes: string[] = [];
let kept = { kept: false, running: false };
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args: { data?: number[] }) => {
    if (cmd === "keep_session_state") return kept;
    if (cmd === "write_terminal_data") writes.push(new TextDecoder().decode(new Uint8Array(args.data!)));
    return null;
  },
}));

const { folderOf, forgetFolder, isAway, lineAt, pathLike, pathToEdit, resolvePath, trackFolder, wordUnder } = await import("../../src/fork/terminal/fileMenu");
const { waitToCd, withCdHere } = await import("../../src/fork/terminal/here");
const { skipIfKept } = await import("../../src/components/KeptSessions");

const term = (cols = 40) => new Terminal({ cols, rows: 6, allowProposedApi: true });
const put = (t: Terminal, s: string) => new Promise<void>((done) => t.write(s, done));

describe("the word under the pointer is the one shown", () => {
  it("counts a wide character as one character in two cells", async () => {
    const t = term();
    await put(t, "日本語 a.txt b.conf\r\n");
    // 日本語 take cells 0-5; a.txt is at 7-11, b.conf at 13-18.
    expect(wordUnder(t, 0, 7)).toBe("a.txt");
    expect(wordUnder(t, 0, 11)).toBe("a.txt");
    expect(wordUnder(t, 0, 12)).toBe("");
    expect(wordUnder(t, 0, 18)).toBe("b.conf");
    // The second cell of a wide character is that character.
    expect(lineAt(t.buffer.active, 0, 1)?.col).toBe(0);
    expect(wordUnder(t, 0, 5)).toBe("日本語");
  });

  it("reads a path wrapped onto the next row as one word, from either row", async () => {
    const t = term(20);
    await put(t, "/var/log/nginx/access.log\r\n");
    expect(wordUnder(t, 0, 3)).toBe("/var/log/nginx/access.log");
    expect(wordUnder(t, 1, 1)).toBe("/var/log/nginx/access.log");
    // A line that isn't wrapped stays apart from the one after it.
    await put(t, "a.txt\r\nb.txt\r\n");
    expect(wordUnder(t, 2, 0)).toBe("a.txt");
    expect(wordUnder(t, 3, 0)).toBe("b.txt");
  });
});

describe("what counts as a path", () => {
  it("not a prompt, another host's path or a web address", () => {
    for (const w of ["paul@box:~/app$", "root@web1:/etc/nginx#", "user@host:/etc/hosts", "www.example.com", "https://x.example/a.txt"]) {
      expect(pathLike(w), w).toBe(false);
    }
    expect(pathLike("/" + "x".repeat(5000))).toBe(false);
    for (const w of ["/etc/systemd/system/getty@tty1.service", "node_modules/@types/x.d.ts", "nginx.conf", ".env", "logs/"]) {
      expect(pathLike(w), w).toBe(true);
    }
  });

  it("in any alphabet", () => {
    for (const w of ["café.txt", "résumé.pdf", "日本.txt", ".конфиг"]) expect(pathLike(w), w).toBe(true);
  });

  it("isn't resolved against a folder with a control character, or past the longest path", () => {
    expect(resolvePath("a.txt", "/home/u\u0085x")).toBeNull();
    expect(resolvePath("a.txt", "/home/u\u0007x")).toBeNull();
    expect(resolvePath("/" + "x".repeat(4096), undefined)).toBeNull();
    expect(resolvePath("a.txt", "/" + "x".repeat(4093))).toBeNull();
    expect(resolvePath("a.txt", "/home/u")).toBe("/home/u/a.txt");
  });

  it("offers no Edit for a folder", () => {
    expect(pathToEdit("logs/", "/var", false)).toBeNull();
    expect(pathToEdit("logs/app.log", "/var", false)).toBe("/var/logs/app.log");
  });
});

describe("Edit only on the tab's own server", () => {
  const osc7 = (host: string, path: string) => `\x1b]7;file://${host}${path}\x07`;
  const iterm = (what: string) => `\x1b]1337;${what}\x07`;
  beforeEach(() => forgetFolder("x"));

  it("edits nothing once the shell reports another host, absolute paths included, until it's back", async () => {
    const t = term();
    trackFolder(t, "x");
    await put(t, osc7("a", "/home/u"));
    expect(folderOf("x")).toBe("/home/u");
    expect(pathToEdit("/etc/nginx/nginx.conf", folderOf("x"), isAway("x"))).toBe("/etc/nginx/nginx.conf");
    await put(t, osc7("b", "/etc"));
    expect(isAway("x")).toBe(true);
    expect(pathToEdit("/etc/nginx/nginx.conf", folderOf("x"), isAway("x"))).toBeNull();
    await put(t, osc7("a", "/srv"));
    expect(folderOf("x")).toBe("/srv");
  });

  it("doesn't take a host-less report after a hosted one for home", async () => {
    const t = term();
    trackFolder(t, "x");
    await put(t, osc7("a", "/home/u") + osc7("b", "/etc") + iterm("CurrentDir=/var/www"));
    expect(folderOf("x")).toBeUndefined();
    expect(isAway("x")).toBe(true);
  });

  it("never makes a host home after host-less reports", async () => {
    const t = term();
    trackFolder(t, "x");
    await put(t, iterm("CurrentDir=/home/u") + osc7("b", "/etc"));
    expect(folderOf("x")).toBeUndefined();
    await put(t, iterm("CurrentDir=/home/u/src"));
    expect(folderOf("x")).toBe("/home/u/src");
  });

  it("takes iTerm2's host from its RemoteHost report", async () => {
    const t = term();
    trackFolder(t, "x");
    await put(t, iterm("RemoteHost=paul@a") + iterm("CurrentDir=/home/u"));
    expect(folderOf("x")).toBe("/home/u");
    await put(t, iterm("RemoteHost=paul@b") + iterm("CurrentDir=/var/www"));
    expect(isAway("x")).toBe(true);
    await put(t, iterm("RemoteHost=paul@a") + iterm("CurrentDir=/srv"));
    expect(folderOf("x")).toBe("/srv");
  });
});

describe("Open terminal here's cd comes after the on-connect commands", () => {
  beforeEach(() => {
    writes.length = 0;
    kept = { kept: false, running: false };
  });

  it("types the cd once the on-connect commands have gone, after the session check", async () => {
    waitToCd("t1", "cd '/var/log'");
    const ready = skipIfKept("t1", async () => {
      await new Promise((r) => setTimeout(r, 5));
      writes.push("cd /app\n");
    });
    await withCdHere("t1", ready)!();
    expect(writes).toEqual(["cd /app\n", "cd '/var/log'\n"]);
    // Once: mounting again doesn't type it twice.
    writes.length = 0;
    await withCdHere("t1", ready)!();
    expect(writes).toEqual(["cd /app\n"]);
  });

  it("waits for the question about held on-connect commands, and still goes when they're not allowed", async () => {
    let answer: (allowed: boolean) => void = () => {};
    const asked = new Promise<boolean>((r) => (answer = r));
    waitToCd("t2", "cd '/srv'");
    const done = withCdHere("t2", skipIfKept("t2", () => asked.then((ok) => void (ok && writes.push("sudo -i\n")))))!();
    await new Promise((r) => setTimeout(r, 5));
    expect(writes).toEqual([]);
    answer(false);
    await done;
    expect(writes).toEqual(["cd '/srv'\n"]);
  });

  it("types the cd in a session that was kept, whose on-connect commands are left out", async () => {
    kept = { kept: true, running: true };
    waitToCd("t3", "cd '/srv'");
    await withCdHere("t3", skipIfKept("t3", () => void writes.push("cd /app\n")))!();
    expect(writes).toEqual(["cd '/srv'\n"]);
  });

  it("lets go of a terminal whose shell never came, once many more wait", async () => {
    waitToCd("gone", "cd '/a'");
    for (let i = 0; i < 40; i++) waitToCd(`w${i}`, "cd '/b'");
    expect(withCdHere("gone", undefined)).toBeUndefined();
    await withCdHere("w39", undefined)!();
    expect(writes).toEqual(["cd '/b'\n"]);
  });
});

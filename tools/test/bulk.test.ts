// Bulk push and pull (#159): the rules in src/fork/bulk/plan.ts.
import { describe, expect, it } from "vitest";
import {
  AT_ONCE, type Check, type LocalEntry, type Step, countLocal, failureOf, finalRow, folderNames, formatBytes, isStepEvent,
  localBase, overwriteFor, pullStep, pushLine, pushSteps, remoteDirArg, remoteItemArg, rowText, runFew, runTotals,
  safeFolderName, summarize, thereLine, throttle,
} from "../../src/fork/bulk/plan";

describe("a server's name as a folder name", () => {
  it("keeps an ordinary name as it is", () => {
    expect(safeFolderName("web-01.example.com")).toBe("web-01.example.com");
    expect(safeFolderName("Web 01 (prod)")).toBe("Web 01 (prod)");
  });

  it("never gives a separator, a drive letter or a way up", () => {
    expect(safeFolderName("../../etc")).toBe("_.._etc");
    expect(safeFolderName("a/b\\c")).toBe("a_b_c");
    expect(safeFolderName("C:")).toBe("C_");
    expect(safeFolderName("..")).toBe("server");
    expect(safeFolderName(".")).toBe("server");
    expect(safeFolderName("")).toBe("server");
    expect(safeFolderName("   ")).toBe("server");
    for (const n of ["../x", "..\\x", "C:\\x", "\\\\host\\share", "a/../../b"]) {
      const s = safeFolderName(n);
      expect(s).not.toMatch(/[\\/:]/);
      expect(s).not.toBe("..");
    }
  });

  it("drops control and invisible characters", () => {
    expect(safeFolderName("web\u0000\u0007\n01")).toBe("web01");
    expect(safeFolderName("we\u200bb\u202e01\ufeff")).toBe("web01");
    expect(safeFolderName("a\tb")).toBe("ab");
  });

  it("replaces what Windows refuses in a name, and the dot or space at the end it drops", () => {
    expect(safeFolderName('a*b?c"d<e>f|g')).toBe("a_b_c_d_e_f_g");
    expect(safeFolderName("web. ")).toBe("web");
    expect(safeFolderName(" .web")).toBe("web");
  });

  it("never gives a Windows device name", () => {
    for (const n of ["CON", "con", "nul.txt", "COM1", "lpt9.log", "AUX .c", "COM¹", "CONIN$"]) {
      expect(safeFolderName(n).startsWith("_")).toBe(true);
    }
    expect(safeFolderName("console")).toBe("console");
    expect(safeFolderName("COM10")).toBe("COM10");
  });

  it("is never longer than its limit", () => {
    expect([...safeFolderName("x".repeat(500))].length).toBe(80);
  });
});

describe("the servers' folder names together", () => {
  it("makes the same name different, ignoring case, in order", () => {
    expect(folderNames(["web", "db", "web", "WEB"])).toEqual(["web", "db", "web (2)", "WEB (3)"]);
  });

  it("leaves a server really called `web (2)` its own name", () => {
    expect(folderNames(["web", "web", "web (2)"])).toEqual(["web", "web (3)", "web (2)"]);
  });

  it("makes names different once they're safe", () => {
    expect(folderNames(["a/b", "a_b", "a:b"])).toEqual(["a_b", "a_b (2)", "a_b (3)"]);
  });
});

describe("remote paths as typed", () => {
  it("takes a folder from home or from the root", () => {
    expect(remoteDirArg("~")).toBe("");
    expect(remoteDirArg(" ~/ ")).toBe("");
    expect(remoteDirArg("~/deploy/")).toBe("deploy");
    expect(remoteDirArg("~/a//b")).toBe("a/b");
    expect(remoteDirArg("/srv/app/")).toBe("/srv/app");
    expect(remoteDirArg("/")).toBe("/");
  });

  it("refuses anything else", () => {
    expect(remoteDirArg("")).toBeNull();
    expect(remoteDirArg("deploy")).toBeNull();
    expect(remoteDirArg("~root/x")).toBeNull();
    expect(remoteDirArg("/tmp/a\nb")).toBeNull();
  });

  it("splits a file or folder into its folder and its name", () => {
    expect(remoteItemArg("~/logs/app.log")).toEqual({ dir: "logs", name: "app.log" });
    expect(remoteItemArg("~/logs/")).toEqual({ dir: "", name: "logs" });
    expect(remoteItemArg("/etc")).toEqual({ dir: "/", name: "etc" });
    expect(remoteItemArg("/var/log/nginx")).toEqual({ dir: "/var/log", name: "nginx" });
  });

  it("has nothing to fetch for a root, home itself, `.` or `..`", () => {
    for (const t of ["/", "~", "~/", "/var/..", "~/.", "relative/x"]) expect(remoteItemArg(t)).toBeNull();
  });
});

describe("local names", () => {
  it("is the last name in the path, on either system", () => {
    expect(localBase("/home/me/deploy/")).toBe("deploy");
    expect(localBase("C:\\Users\\me\\deploy")).toBe("deploy");
    expect(localBase("/home/me/a\\b")).toBe("a\\b");
    expect(localBase("/")).toBe("");
    expect(localBase("C:\\")).toBe("");
  });
});

describe("the steps", () => {
  const picks = [
    { path: "/home/me/site/index.html", name: "index.html", isDir: false },
    { path: "/home/me/site/assets", name: "assets", isDir: true },
  ];

  it("sends a file to its full path and a folder to the folder above it", () => {
    expect(pushSteps(picks, "/srv/www", new Set(["assets"]))).toEqual([
      { cmd: "sftp_upload_file", localPath: "/home/me/site/index.html", remotePath: "/srv/www/index.html", name: "index.html", there: false },
      { cmd: "sftp_upload_dir", localPath: "/home/me/site/assets", remotePath: "/srv/www", name: "assets", there: true },
    ]);
    expect(pushSteps(picks.slice(0, 1), "/", new Set())[0].remotePath).toBe("/index.html");
  });

  it("fetches into the server's own folder here", () => {
    expect(pullStep("/var/log", "syslog", false, "/home/me/pulled/web", true)).toEqual({
      cmd: "sftp_download_file", localPath: "/home/me/pulled/web/syslog", remotePath: "/var/log/syslog", name: "syslog", there: true,
    });
    expect(pullStep("/var/log", "nginx", true, "C:\\pulled\\web", false)).toEqual({
      cmd: "sftp_download_dir", localPath: "C:\\pulled\\web", remotePath: "/var/log/nginx", name: "nginx", there: false,
    });
    expect(pullStep("/", "etc", true, "/p/w", false).remotePath).toBe("/etc");
  });

  it("knows its own progress events by kind and name", () => {
    const up = pushSteps(picks, "/srv", new Set())[1];
    expect(isStepEvent(up, { kind: "upload", name: "assets" })).toBe(true);
    expect(isStepEvent(up, { kind: "download", name: "assets" })).toBe(false);
    expect(isStepEvent(up, { kind: "upload", name: "other" })).toBe(false);
  });
});

describe("skip or replace", () => {
  const step = (there: boolean): Step => ({ cmd: "sftp_upload_file", localPath: "/a", remotePath: "/b/a", name: "a", there });

  it("replaces only what was there, and only when the user said so", () => {
    expect(overwriteFor(step(false), "skip")).toBe(false);
    expect(overwriteFor(step(false), "replace")).toBe(false);
    expect(overwriteFor(step(true), "skip")).toBeNull();
    expect(overwriteFor(step(true), "replace")).toBe(true);
  });

  it("counts what turned up since the check as skipped, a cancel as stopped, the rest as failed", () => {
    expect(failureOf("EXISTS:/srv/a")).toEqual({ end: "skipped" });
    expect(failureOf("cancelled")).toEqual({ end: "stopped" });
    expect(failureOf("Permission denied")).toEqual({ end: "failed", reason: "Permission denied" });
    expect(failureOf(new Error("gone"))).toEqual({ end: "failed", reason: "gone" });
  });
});

describe("the summary and the rows", () => {
  const s = (there: boolean): Step => ({ cmd: "sftp_upload_file", localPath: "/a", remotePath: "/b/a", name: "a", there });
  const checks: Check[] = [
    { ok: true, steps: [s(true), s(true)] },
    { ok: true, steps: [s(false), s(true)] },
    { ok: true, steps: [s(false)] },
    { ok: false, reason: "No such folder" },
  ];

  it("counts the servers, those left out, and what's there already", () => {
    expect(summarize(checks)).toEqual({ servers: 3, problems: 1, there: 3, thereServers: 2 });
    expect(thereLine(summarize(checks), "push")).toBe("3 of these are already there, on 2 servers.");
    expect(thereLine({ servers: 1, problems: 0, there: 1, thereServers: 1 }, "pull")).toBe("1 of these is already there on this computer.");
  });

  it("says what a push sends", () => {
    expect(pushLine({ files: 3, bytes: 5 * 1024 * 1024, more: false }, 5, "~/deploy")).toBe("3 files (5.0 MB) to 5 servers, into ~/deploy.");
    expect(pushLine({ files: 1, bytes: 10, more: true }, 1, "/srv")).toBe("At least 1 file (10 bytes) to 1 server, into /srv.");
    expect(formatBytes(1536)).toBe("1.5 KB");
  });

  it("calls a server where everything was there skipped", () => {
    expect(finalRow(0, 2)).toEqual({ state: "skipped" });
    expect(finalRow(1, 1)).toEqual({ state: "done", copied: 1, skipped: 1 });
    expect(rowText(finalRow(1, 1))).toBe("Done, 1 already there left as they were");
    expect(rowText({ state: "copying", step: 2, steps: 3, pct: 40 })).toBe("Copying 40% (2 of 3)");
    expect(rowText({ state: "copying", step: 1, steps: 1, pct: null })).toBe("Copying");
    expect(runTotals([finalRow(1, 0), { state: "skipped" }, { state: "failed", reason: "x" }, { state: "stopped" }, { state: "queued" }]))
      .toBe("1 done, 1 skipped, 1 failed, 2 stopped.");
  });
});

describe("counting what a push sends", () => {
  const tree: Record<string, LocalEntry[]> = {
    "/s/a": [
      { path: "/s/a/one", name: "one", is_dir: false, size: 10 },
      { path: "/s/a/sub", name: "sub", is_dir: true, size: 0 },
      { path: "/s/a/link", name: "link", is_dir: true, size: 0, is_symlink: true },
      { path: "/s/a/flink", name: "flink", is_dir: false, size: 99, is_symlink: true },
    ],
    "/s/a/sub": [{ path: "/s/a/sub/two", name: "two", is_dir: false, size: 5 }],
  };
  const list = async (p: string) => tree[p] ?? [];

  it("counts files and bytes, without following links", async () => {
    const picks = [{ path: "/s/f", name: "f", isDir: false, size: 7 }, { path: "/s/a", name: "a", isDir: true, size: 0 }];
    expect(await countLocal(picks, list)).toEqual({ files: 3, bytes: 22, more: false });
  });

  it("stops at its limit and says it's at least that", async () => {
    const endless = async (p: string): Promise<LocalEntry[]> => [{ path: `${p}/d`, name: "d", is_dir: true, size: 0 }, { path: `${p}/f`, name: "f", is_dir: false, size: 1 }];
    const c = await countLocal([{ path: "/x", name: "x", isDir: true, size: 0 }], endless, 50);
    expect(c.more).toBe(true);
    expect(c.files).toBe(50);
  });

  it("counts on past a folder it can't read", async () => {
    const failing = async (p: string) => { if (p === "/s/a/sub") throw new Error("denied"); return tree[p] ?? []; };
    expect(await countLocal([{ path: "/s/a", name: "a", isDir: true, size: 0 }], failing)).toEqual({ files: 1, bytes: 10, more: false });
  });
});

describe("a few servers at a time", () => {
  const tick = () => new Promise((r) => setTimeout(r, 1));

  it("never runs more than its limit at once, and runs them all", async () => {
    let now = 0;
    let most = 0;
    const done: number[] = [];
    await runFew(Array.from({ length: 10 }, (_, i) => i), AT_ONCE, async (i) => {
      now++;
      most = Math.max(most, now);
      await tick();
      now--;
      done.push(i);
    });
    expect(most).toBe(AT_ONCE);
    expect(done.sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it("goes on past one that fails", async () => {
    const done: number[] = [];
    await runFew([0, 1, 2], 1, async (i) => { if (i === 1) throw new Error("x"); done.push(i); });
    expect(done).toEqual([0, 2]);
  });

  it("starts nothing more once stopped", async () => {
    let stopped = false;
    const started: number[] = [];
    await runFew(Array.from({ length: 10 }, (_, i) => i), 2, async (i) => {
      started.push(i);
      if (i === 2) stopped = true;
      await tick();
    }, () => stopped);
    expect(started).toEqual([0, 1, 2]);
  });
});

describe("progress on the page", () => {
  it("redraws at most once a period however many events come", () => {
    const pending: (() => void)[] = [];
    let flushed = 0;
    const poke = throttle(() => { flushed++; }, 250, (f) => { pending.push(f); });
    for (let i = 0; i < 100; i++) poke();
    expect(pending.length).toBe(1);
    expect(flushed).toBe(0);
    pending.shift()!();
    expect(flushed).toBe(1);
    poke();
    expect(pending.length).toBe(1);
  });

  it("does nothing while nothing pokes", () => {
    const pending: (() => void)[] = [];
    throttle(() => {}, 250, (f) => { pending.push(f); });
    expect(pending.length).toBe(0);
  });
});

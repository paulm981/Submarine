// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).

import { describe, expect, it } from "vitest";
import { aiMarker, cardsOf, copyCard, levelTitle, runnerWarning, sharedWith, trustTargetFor, type AiCopy, type AiPending, type AiServer, type AiSnapshot } from "../../src/util/ai";

const pending = (id: number, group: { id: number; size: number } | null = null): AiPending => ({
  id, tab_id: "t", tab_label: "AI", session_id: `s${id}`, server: `srv${id}`, user: null, command: "uptime",
  root: false, as_root: false, root_via: null, password: "none", timeout_secs: 60, asked_at: "", server_id: null,
  ask_rule: null, can_allow_always: true, note: null, group, copy: null,
});

const server = (over: Partial<AiServer> = {}): AiServer => ({
  session_id: "s1", name: "web", server_id: 7, allow_all: false, level: 1, root_via: null, ...over,
});

describe("cardsOf", () => {
  it("shows one card per request when none is grouped", () => {
    const cards = cardsOf([pending(1), pending(2)]);
    expect(cards.map((c) => c.p.id)).toEqual([1, 2]);
    expect(cards.every((c) => c.members.length === 1)).toBe(true);
  });

  it("puts a request on several servers on one card, where its first is", () => {
    const g = { id: 9, size: 3 };
    const cards = cardsOf([pending(1, g), pending(2), pending(3, g), pending(4, g)]);
    expect(cards.map((c) => c.p.id)).toEqual([1, 2]);
    expect(cards[0].members.map((m) => m.id)).toEqual([1, 3, 4]);
  });

  it("keeps two groups apart", () => {
    const cards = cardsOf([pending(1, { id: 1, size: 2 }), pending(2, { id: 2, size: 2 }), pending(3, { id: 1, size: 2 })]);
    expect(cards.map((c) => c.members.map((m) => m.id))).toEqual([[1, 3], [2]]);
  });

  it("shows nothing for nothing", () => {
    expect(cardsOf([])).toEqual([]);
  });
});

describe("copyCard", () => {
  const push: AiCopy = {
    direction: "push", source: "/home/me/app", destination: "~/releases", replace: false, into: null,
    local: { is_dir: true, files: 1200, bytes: 3 * 1024 * 1024, folders: 4, links_skipped: 0, others_skipped: 0 },
  };
  const row = (c: AiCopy, label: string) => copyCard(c).rows.find((r) => r.label === label)?.value;

  it("says what a push sends, from where, to where", () => {
    const card = copyCard(push);
    expect(card.wants).toBe("wants to copy files to");
    expect(row(push, "From this computer")).toBe("/home/me/app");
    expect(row(push, "To, on each server")).toBe("~/releases");
    expect(row(push, "What")).toBe(`A folder: ${(1200).toLocaleString()} files, 3.0 MB`);
    expect(row(push, "Already there")).toBe("Left as they are");
    expect(card.rows.filter((r) => r.path).map((r) => r.value)).toEqual(["/home/me/app", "~/releases"]);
    expect(card.notes).toEqual([]);
  });

  it("says when files are replaced, and when links are left out", () => {
    const one = { ...push, replace: true, local: { ...push.local!, is_dir: false, files: 1, bytes: 5, links_skipped: 2 } };
    expect(row(one, "What")).toBe("1 file, 5 bytes");
    expect(row(one, "Already there")).toBe("Replaced");
    expect(copyCard(one).notes).toEqual([
      "Files of the same name already on the servers are replaced.",
      "2 links inside are left out: links on this computer aren't followed.",
    ]);
  });

  it("says a pull finds what it copies when it runs, and replaces nothing here", () => {
    const pull: AiCopy = { direction: "pull", source: "/var/log/nginx", destination: "/home/me/AI", replace: false, local: null, into: "/home/me/AI/web" };
    const card = copyCard(pull);
    expect(card.wants).toBe("wants to copy files from");
    expect(row(pull, "From each server")).toBe("/var/log/nginx");
    expect(row(pull, "To this computer")).toBe("/home/me/AI");
    expect(row(pull, "What")).toBe("Found when it runs");
    expect(row(pull, "Already there")).toContain("nothing on this computer is replaced");
    expect(card.notes[0]).toContain("a folder of its own");
  });
});

describe("runnerWarning", () => {
  it("warns about a wildcard after a command that runs other commands", () => {
    for (const p of ["timeout *", "env*", "  xargs  * ", "bash -c *","kubectl exec *"]) {
      expect(runnerWarning(p), p).toContain("lets the AI run any command");
    }
  });

  it("sees through a path, quotes, letter case and a Windows extension", () => {
    for (const p of ["/usr/bin/env *", `"bash" *`, "C:\\Windows\\System32\\cmd.exe *", "PowerShell *", "python3 *", "python3.12 -c *"]) {
      expect(runnerWarning(p), p).not.toBeNull();
    }
  });

  it("says nothing for an ordinary command, or a rule without a wildcard", () => {
    for (const p of ["ls *", "systemctl status *", "timeout 5 uptime", "bash", "*", "   ", "cat /var/log/*"]) {
      expect(runnerWarning(p), p).toBeNull();
    }
  });

  it("quotes the rule as typed, trimmed", () => {
    expect(runnerWarning("  nohup * ")).toBe("`nohup *` lets the AI run any command, as root too where sudo needs no password.");
  });
});

describe("approval levels", () => {
  it("marks a tab only where commands run without asking", () => {
    expect(aiMarker({ level: 1, allow_all: false })).toBeNull();
    expect(aiMarker({ level: 2, allow_all: false })).toBe("AI: allowed commands run");
    expect(aiMarker({ level: 3, allow_all: false })).toBe("AI: runs without asking");
    // "Allow everything here" outranks the level.
    expect(aiMarker({ level: 1, allow_all: true })).toBe("AI: runs without asking");
  });

  it("names a level, and an unknown one as the safest", () => {
    expect(levelTitle(2)).toBe("Run my allowed commands, ask for the rest");
    expect(levelTitle(0)).toBe("Ask every time");
    expect(levelTitle(99)).toBe("Ask every time");
  });

  it("edits a saved server's own level, and the default for a quick connect", () => {
    expect(trustTargetFor(server())).toEqual({ scope: "server", id: 7, name: "web" });
    expect(trustTargetFor(server({ server_id: null }))).toEqual({ scope: "default", id: 0, name: "every server" });
  });
});

describe("sharedWith", () => {
  it("lists the AI tabs a server's tab is shared with", () => {
    const snap: AiSnapshot = {
      enabled: true, pending: [], running: [], history: [], audit_log: null,
      tabs: [
        { id: "a", label: "A", intruder_at: null, servers: [server({ session_id: "s1" })] },
        { id: "b", label: "B", intruder_at: null, servers: [server({ session_id: "s2" })] },
        { id: "c", label: "C", intruder_at: null, servers: [server({ session_id: "s2" }), server({ session_id: "s1" })] },
      ],
    };
    expect(sharedWith(snap, "s1").map((t) => t.id)).toEqual(["a", "c"]);
    expect(sharedWith(snap, "s3")).toEqual([]);
  });
});

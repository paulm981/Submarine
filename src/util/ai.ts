import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { formatBytes } from "../fork/bulk/plan";

// AI help on open sessions (#32, src-tauri/src/ai/mod.rs). The backend holds
// all of it: the AI tabs, what's shared with each, the requests waiting for
// you, what ran. The page shows it (the AI panel, Share with AI) and passes
// on your clicks; `ai-changed` says when to look again.

export interface AiServer {
  session_id: string; name: string;
  /// The saved server its tab was opened for; null for a quick connect.
  server_id: number | null;
  /// "Allow everything here until this AI tab closes" is on.
  allow_all: boolean;
  /// Its approval level now (1, 2 or 3).
  level: number;
  /// What root goes through there for this AI tab, once a request as root
  /// worked (#120): "sudo" or "dzdo".
  root_via: string | null;
}
export interface AiTab {
  id: string; label: string; servers: AiServer[];
  /// When another program tried to connect as this AI tab (and was refused).
  intruder_at: string | null;
}
export interface AiPending {
  id: number; tab_id: string; tab_label: string; session_id: string; server: string;
  /// Who it runs as: the server tab's login user, if known.
  user: string | null;
  command: string;
  /// It runs as root: the login user is root, the command asks for root,
  /// or the AI asked to run it as root.
  root: boolean;
  /// The AI asked to run it as root (`as_root`, #120).
  as_root: boolean;
  /// What it goes through as root, once known: "sudo" or "dzdo".
  root_via: string | null;
  /// For a request as root: whether the card asks for the root password.
  /// "optional" while nothing is known (left empty for a server that
  /// doesn't ask), "needed" once the server asked, "wrong" when the one
  /// typed didn't work.
  password: "none" | "optional" | "needed" | "wrong";
  timeout_secs: number; asked_at: string;
  server_id: number | null;
  /// The always-ask rule that made it wait, if one did.
  ask_rule: string | null;
  /// "Always allow this command here" is offered for it.
  can_allow_always: boolean;
  /// Why it asks where the server's settings otherwise wouldn't: they
  /// couldn't be read, or a change to them waits for your OK.
  note: string | null;
  /// One of a request's servers, when the AI asked for one command on
  /// several (#158): shown as one card, answered together.
  group: { id: number; size: number } | null;
  /// A copy_files request (#159): what it copies, where. `command` is then
  /// a one-line summary of it.
  copy: AiCopy | null;
  /// A follow request (#179): it keeps running until stopped, at most
  /// `timeout_secs`.
  follow: boolean;
}

/// What a copy_files request copies (src-tauri/src/ai/copy.rs, CopyView).
export interface AiCopy {
  direction: "push" | "pull";
  /// Push: the path on this computer; pull: the path on each server.
  source: string;
  /// Push: the folder on each server; pull: the folder on this computer.
  destination: string;
  /// Push: what's sent, counted on this computer. Pull: null, found when it
  /// runs.
  local: { is_dir: boolean; files: number; bytes: number; folders: number; links_skipped: number; others_skipped: number } | null;
  /// Push: files already on the server are replaced.
  replace: boolean;
  /// Pull: the folder of its own this server's copy goes in.
  into: string | null;
}

const count = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/// A copy's card, in words: what it "wants to" do, and its rows (a path's
/// row is `path`, shown as it is, hidden characters marked), and anything
/// to warn about.
export const copyCard = (c: AiCopy): { wants: string; rows: { label: string; value: string; path?: boolean }[]; notes: string[] } => {
  const push = c.direction === "push";
  const rows: { label: string; value: string; path?: boolean }[] = push
    ? [
        { label: "From this computer", value: c.source, path: true },
        { label: "To, on each server", value: c.destination, path: true },
        {
          label: "What",
          value: c.local
            ? `${c.local.is_dir ? "A folder: " : ""}${count(c.local.files, "file")}, ${formatBytes(c.local.bytes)}`
            : "Counted when it runs",
        },
        { label: "Already there", value: c.replace ? "Replaced" : "Left as they are" },
      ]
    : [
        { label: "From each server", value: c.source, path: true },
        { label: "To this computer", value: c.destination, path: true },
        { label: "What", value: "Found when it runs" },
        { label: "Already there", value: "Left as they are: nothing on this computer is replaced" },
      ];
  const notes: string[] = [];
  if (!push) notes.push("Each server's copy goes in a folder of its own inside it, named after the server.");
  if (push && c.replace) notes.push("Files of the same name already on the servers are replaced.");
  const links = c.local?.links_skipped ?? 0;
  if (links > 0) notes.push(`${count(links, "link")} inside ${links === 1 ? "is" : "are"} left out: links on this computer aren't followed.`);
  return { wants: push ? "wants to copy files to" : "wants to copy files from", rows, notes };
};


/// A follow's card line (#179): it keeps running.
export const followLimit = (p: Pick<AiPending, "timeout_secs">): string => {
  const hours = p.timeout_secs / 3600;
  return `until stopped (at most ${Number.isInteger(hours) ? `${hours} h` : `${Math.round(p.timeout_secs / 60)} min`})`;
};

/// How long a follow has run, and what it's seen, for the panel.
export const followSoFar = (startedAt: string, bytesSeen: number, now: number): string => {
  const since = new Date(startedAt).getTime();
  const mins = isNaN(since) ? 0 : Math.max(0, Math.floor((now - since) / 60000));
  const age = mins < 1 ? "under a minute" : mins < 60 ? `${mins} min` : `${Math.floor(mins / 60)} h ${mins % 60} min`;
  return `${age} · ${formatBytes(bytesSeen)} seen`;
};

/// The cards to show for `pending`: one per request, a request on several
/// servers (#158) as one card with its servers as `members`, where its
/// first is.
export const cardsOf = (pending: AiPending[]): { p: AiPending; members: AiPending[] }[] => {
  const out: { p: AiPending; members: AiPending[] }[] = [];
  for (const p of pending) {
    const group = p.group?.id;
    const card = group == null ? undefined : out.find((c) => c.p.group?.id === group);
    if (card) card.members.push(p);
    else out.push({ p, members: [p] });
  }
  return out;
};
export interface AiRunning {
  id: number; tab_label: string; session_id: string; server: string; command: string; started_at: string;
  /// Why it runs without your Allow, if it does.
  ran_without_asking: string | null;
  /// It runs as root at the AI's asking, and through what.
  as_root: boolean; root_via: string | null;
  /// A follow (#179): what it's seen so far. Stopped with `ai_stop_follow`.
  follow: { bytes_seen: number } | null;
}
export interface AiDone {
  id: number; tab_label: string; server: string; command: string; root: boolean;
  /// It ran as root at the AI's asking, and through what.
  as_root: boolean; root_via: string | null;
  outcome: "denied" | "cancelled" | "done" | "failed" | "stopped";
  exit_code: number | null; output_bytes: number; hidden: number;
  cut: boolean; timed_out: boolean; error: string | null; at: string;
  /// It ran without asking you, and why.
  ran_without_asking: string | null;
  /// A follow (#179): how it ended and what it saw. Its output isn't kept.
  follow: { ended: string; bytes_seen: number } | null;
}
/// The AI asking for a server by name (src-tauri/src/ai/ask_server.rs): the
/// saved server it found, if exactly one has that name.
export interface AiServerAsk {
  id: number; tab_id: string; tab_label: string; asked: string;
  server_id: number | null; server_name: string | null; many: boolean; asked_at: string;
}

export interface AiSnapshot {
  enabled: boolean; tabs: AiTab[]; pending: AiPending[]; server_asks: AiServerAsk[]; running: AiRunning[]; history: AiDone[];
  audit_log: string | null;
}

const EMPTY: AiSnapshot = { enabled: false, tabs: [], pending: [], server_asks: [], running: [], history: [], audit_log: null };

/// Asks the session tree to open a saved server's tab (or show it, if it's
/// open): Share on a request for a server. The tree's own opening holds
/// back a server a teammate moved, as a click there would.
export const OPEN_SERVER = "submarine-ai-open-server";
export const openServerTab = (serverId: number) => window.dispatchEvent(new CustomEvent(OPEN_SERVER, { detail: serverId }));

// One copy of the state for the whole page, however many components use it:
// one `ai_state` call per change, not one per open server tab.
let current: AiSnapshot = EMPTY;
const watchers = new Set<() => void>();
let stopListening: (() => void) | null = null;
// Only the newest answer counts, should two cross.
let asked = 0;

const load = () => {
  const n = ++asked;
  invoke<AiSnapshot>("ai_state")
    .then((s) => {
      if (n !== asked) return;
      current = s;
      watchers.forEach((w) => w());
    })
    .catch(() => {});
};

const subscribe = (w: () => void) => {
  watchers.add(w);
  if (!stopListening) {
    let alive = true;
    const un = listen("ai-changed", load);
    stopListening = () => { alive = false; un.then((u) => u()); };
    // Once listening, again: a change before then would be missed.
    void un.then(() => { if (alive) load(); });
  }
  return () => {
    watchers.delete(w);
    if (watchers.size === 0 && stopListening) {
      stopListening();
      stopListening = null;
    }
  };
};

/// The AI state, kept current.
export const useAi = (): AiSnapshot => useSyncExternalStore(subscribe, () => current);

/// The AI tabs a server's tab is shared with.
export const sharedWith = (snap: AiSnapshot, sessionId: string): AiTab[] =>
  snap.tabs.filter((t) => t.servers.some((s) => s.session_id === sessionId));

/// The AI tools this computer can start (Home's menu, under AI tools): none
/// while AI tools are off or none is installed.
export const findAiTools = (): Promise<{ id: string; label: string }[]> =>
  invoke<{ ai_tools?: { id: string; label: string }[] }>("list_local_shells")
    .then((s) => s.ai_tools ?? [])
    .catch(() => []);

// ── Approval levels (#115, src-tauri/src/ai/trust.rs) ───────────────────────
// How much the AI may run on a server without asking. Set on a server, a
// folder or for every server, on this device only; only you change them.

/// Beside "Run anything" and "Allow everything here" (#178): what runs
/// without asking can be steered by what a server prints.
export const STEERED_BY_OUTPUT =
  "Anything a server prints can steer the AI, and this runs what that text talks it into without asking you.";

/// The levels as stored (src-tauri/src/ai/trust.rs, Level): "Run commands
/// that only read" (#190) is 4, though it sits between 2 and 3.
export const READ_ONLY_LEVEL = 4;

/// The levels in the order they're shown, least allowed first.
export const AI_LEVELS: { level: number; title: string; text: string }[] = [
  { level: 1, title: "Ask every time", text: "Every command waits for your Allow." },
  {
    level: 2,
    title: "Run my allowed commands, ask for the rest",
    text: "Commands on your allow list run straight away. Anything else waits for your Allow, and so does a command with sudo, su, doas, dzdo or pkexec, and every request to run something as root.",
  },
  {
    level: READ_ONLY_LEVEL,
    title: "Run commands that only read, ask before changes",
    text: "Commands that only look run straight away: ls, cat, grep, ps, df, journalctl, systemctl status, docker ps and logs, and others Submarine knows only read, also joined with |. So do the commands on your allow list. Anything else waits for your Allow: changes, anything that reaches out (curl, ping), redirection, root, copies, and reading files that usually hold secrets, such as .env or .ssh. On a Windows server, only your allow list runs. Anything a server prints can still steer what the AI reads.",
  },
  {
    level: 3,
    title: "Run anything, just log it",
    text: "Every command runs without asking, including sudo where the server doesn't ask for a password. A request to run something as root runs once you've typed the root password for that AI tab, or where the server needs none. Your always-ask list still asks. Everything is in the AI panel and the audit log, and Stop still works. " + STEERED_BY_OUTPUT,
  },
];

export const levelTitle = (level: number) => AI_LEVELS.find((l) => l.level === level)?.title ?? AI_LEVELS[0].title;

/// What a shared server's tab and the AI pane say about it: nothing at
/// "Ask every time".
export const aiMarker = (s: Pick<AiServer, "level" | "allow_all">): string | null =>
  s.allow_all || s.level === 3
    ? "AI: runs without asking"
    : s.level === READ_ONLY_LEVEL
      ? "AI: read-only commands run"
      : s.level === 2
        ? "AI: allowed commands run"
        : null;

/// What a teammate's move held for your OK lets the AI do here, for Waiting
/// for your OK (HeldReview.tsx; src-tauri/src/ai/trust.rs, HeldPolicy).
export const heldMay = (move: { level: number; allow: number; change?: string }): string =>
  move.level === 3
    ? "the AI may run anything without asking"
    : move.level === READ_ONLY_LEVEL
      ? "the AI may run commands that only read without asking"
      : move.change === "added"
        ? "the AI may run your allowed commands without asking"
        : move.allow > 0
          ? `the AI may run ${move.allow} more allowed command${move.allow === 1 ? "" : "s"} without asking`
          : "the AI may do more without asking";

/// How much a share lets the AI run without asking: higher is more. A
/// level it doesn't know counts as "Ask every time".
const looseness = (s: Pick<AiServer, "level" | "allow_all">): number =>
  s.allow_all ? 4 : Math.max(0, AI_LEVELS.findIndex((l) => l.level === s.level));

/// Of a server's shares with AI tabs, the one that lets the AI run the most
/// without asking; null when none runs anything without asking.
export const loosestShare = <T extends Pick<AiServer, "level" | "allow_all">>(shares: T[]): T | null =>
  shares.reduce<T | null>((best, s) => (looseness(s) > (best ? looseness(best) : 0) ? s : best), null);

/// What the editor is for: a saved server, a folder, or every server.
export interface AiTrustTarget { scope: "server" | "folder" | "default"; id: number; name: string }

// The editor's window, opened from anywhere (a tab's marker, the AI pane, the
// session tree's menus) and shown once, by DesktopApp.
let trustTarget: AiTrustTarget | null = null;
const trustWatchers = new Set<() => void>();
const setTrustTarget = (t: AiTrustTarget | null) => {
  trustTarget = t;
  trustWatchers.forEach((w) => w());
};
export const openAiTrust = (t: AiTrustTarget) => setTrustTarget(t);
export const closeAiTrust = () => setTrustTarget(null);
export const useAiTrustTarget = (): AiTrustTarget | null =>
  useSyncExternalStore((w) => { trustWatchers.add(w); return () => { trustWatchers.delete(w); }; }, () => trustTarget);

/// The editor for the server behind a shared tab: its saved server, or the
/// default for one that isn't saved.
export const trustTargetFor = (s: AiServer): AiTrustTarget =>
  s.server_id !== null ? { scope: "server", id: s.server_id, name: s.name } : { scope: "default", id: 0, name: "every server" };

// The commands that run other commands they're given (trust.rs's RUNNERS,
// kept the same): an allow rule with a wildcard that starts with one allows
// any command. The backend refuses to save one without the extra click.
const RUNNERS = [
  "env", "xargs", "timeout", "nice", "ionice", "nohup", "exec", "command", "builtin", "busybox", "stdbuf", "setsid",
  "chroot", "watch", "sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh", "fish", "python", "perl", "ruby", "node", "php",
  "awk", "gawk", "mawk", "find", "git", "ssh", "rsync", "tar", "docker", "kubectl", "cmd", "powershell", "pwsh",
  "sed", "time", "strace", "ltrace", "gdb", "nsenter", "unshare", "systemd-run", "flock", "parallel", "script", "sshpass",
  "expect", "screen", "tmux", "at", "batch", "iex", "invoke-expression", "start-process", "wsl",
];

/// The warning for an allow rule like `timeout *`; null for any other.
export const runnerWarning = (pattern: string): string | null => {
  const p = pattern.trim();
  if (!p.endsWith("*")) return null;
  const first = p.replace(/\*+$/, "").split(/[\s;&|`$><(){}]+/).find((w) => w !== "");
  if (!first) return null;
  const word = first.replace(/['"\\]/g, (c) => (c === "\\" ? "\\" : "")).toLowerCase();
  const base = word.split(/[/\\]/).pop() ?? word;
  const stem = base.replace(/\.(exe|com|bat|cmd)$/, "");
  const runs = [word, base, stem].some((f) => RUNNERS.some((r) => f === r || (r === "python" && f.startsWith(r))));
  return runs ? `\`${p}\` lets the AI run any command, as root too where sudo needs no password.` : null;
};

import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  /// It runs as root: the login user is root, or the command asks for root.
  root: boolean;
  timeout_secs: number; asked_at: string;
  server_id: number | null;
  /// The always-ask rule that made it wait, if one did.
  ask_rule: string | null;
  /// "Always allow this command here" is offered for it.
  can_allow_always: boolean;
  /// Why it asks where the server's settings otherwise wouldn't: they
  /// couldn't be read, or a change to them waits for your OK.
  note: string | null;
}
export interface AiRunning {
  id: number; tab_label: string; session_id: string; server: string; command: string; started_at: string;
  /// Why it runs without your Allow, if it does.
  ran_without_asking: string | null;
}
export interface AiDone {
  id: number; tab_label: string; server: string; command: string; root: boolean;
  outcome: "denied" | "cancelled" | "done" | "failed" | "stopped";
  exit_code: number | null; output_bytes: number; hidden: number;
  cut: boolean; timed_out: boolean; error: string | null; at: string;
  /// It ran without asking you, and why.
  ran_without_asking: string | null;
}
export interface AiSnapshot {
  enabled: boolean; tabs: AiTab[]; pending: AiPending[]; running: AiRunning[]; history: AiDone[];
  audit_log: string | null;
}

const EMPTY: AiSnapshot = { enabled: false, tabs: [], pending: [], running: [], history: [], audit_log: null };

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

export const AI_LEVELS: { level: number; title: string; text: string }[] = [
  { level: 1, title: "Ask every time", text: "Every command waits for your Allow." },
  {
    level: 2,
    title: "Run my allowed commands, ask for the rest",
    text: "Commands on your allow list run straight away. Anything else waits for your Allow, and so does a command with sudo, su, doas, dzdo or pkexec.",
  },
  {
    level: 3,
    title: "Run anything, just log it",
    text: "Every command runs without asking, including sudo where the server doesn't ask for a password. Your always-ask list still asks. Everything is in the AI panel and the audit log, and Stop still works.",
  },
];

export const levelTitle = (level: number) => AI_LEVELS.find((l) => l.level === level)?.title ?? AI_LEVELS[0].title;

/// What a shared server's tab and the AI pane say about it: nothing at
/// "Ask every time".
export const aiMarker = (s: Pick<AiServer, "level" | "allow_all">): string | null =>
  s.allow_all || s.level >= 3 ? "AI: runs without asking" : s.level === 2 ? "AI: allowed commands run" : null;

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

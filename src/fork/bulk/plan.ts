// ─────────────────────────────────────────────────────────────────────────────
// Bulk push and pull (#159): the rules, with nothing here touching the disk,
// a server or the page. The dialog (BulkDialog.tsx) runs them over the app's
// own single-server transfers (sftp_upload_file, sftp_upload_dir,
// sftp_download_file, sftp_download_dir), which keep their own checks: local
// paths through guard_local_path, and nothing replaced without
// `overwrite: true` (an `EXISTS:<path>` error otherwise).
//
// Kept apart from the dialog so an AI tool (#159's step 2) can drive the same
// steps later.
// ─────────────────────────────────────────────────────────────────────────────

import { localJoin } from "../../fs/localPath";

export type Mode = "push" | "pull";

/** How many servers copy at once. */
export const AT_ONCE = 4;

/** At most this many progress updates a second reach the page. */
export const PROGRESS_MS = 250;

// ── Server names as folder names ─────────────────────────────────────────────

// Device names Windows keeps for itself, whatever follows the first dot.
const RESERVED = new Set([
  "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$",
  ..."0123456789¹²³".split("").flatMap((d) => [`COM${d}`, `LPT${d}`]),
]);

/** Longest folder name made from a server's name, in characters. */
export const NAME_MAX = 80;

/** A server's name as one safe folder name on any system: no separators,
 *  drive letters, `..`, control or invisible characters, no Windows device
 *  name, no dot or space at its end. What can't be kept becomes `_`; nothing
 *  left at all becomes "server". */
export const safeFolderName = (name: string): string => {
  let s = name.normalize("NFC");
  // Control characters, and the invisible ones (zero-width, direction marks).
  s = s.replace(/[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/gu, "");
  // Separators and the characters Windows refuses in a name.
  s = s.replace(/[\\/:*?"<>|]/g, "_");
  s = s.replace(/\s+/g, " ");
  s = [...s].slice(0, NAME_MAX).join("");
  // Windows drops a dot or a space at the end; a name of dots is `.` or `..`.
  s = s.replace(/^[ .]+|[ .]+$/g, "");
  if (!s) return "server";
  const stem = s.split(".")[0].trimEnd().toUpperCase();
  if (RESERVED.has(stem)) s = `_${s}`;
  return s;
};

/** Each server's folder name: safe, and different from the others' however
 *  the system compares names (Windows and macOS ignore case): the second
 *  `web` is `web (2)`. In the order given. */
export const folderNames = (names: string[]): string[] => {
  const taken = new Set<string>();
  const key = (s: string) => s.toLowerCase();
  // A name as given counts as taken before any ` (n)` is made up, so a server
  // really called `web (2)` keeps its own name.
  const wanted = names.map(safeFolderName);
  const asGiven = new Set(wanted.map(key));
  return wanted.map((base, i) => {
    const firstOfIt = wanted.findIndex((w) => key(w) === key(base)) === i;
    let name = base;
    for (let n = 2; taken.has(key(name)) || (!firstOfIt && asGiven.has(key(name))); n++) name = `${base} (${n})`;
    taken.add(key(name));
    return name;
  });
};

// ── Remote paths, as the user types them ─────────────────────────────────────

/** A remote folder as typed: `~` (the login's home folder), `~/…` or a full
 *  path. What sftp_list_dir takes for it: a path from home has no `~` (a
 *  relative path starts at home), and "" is home itself. null: not a path
 *  this takes. */
export const remoteDirArg = (typed: string): string | null => {
  const t = typed.trim();
  if (!t || /[\p{Cc}]/u.test(t)) return null;
  const tidy = (p: string) => p.replace(/\/{2,}/g, "/");
  if (t === "~" || t === "~/") return "";
  if (t.startsWith("~/")) return tidy(t.slice(2)).replace(/\/+$/, "");
  if (t.startsWith("/")) return tidy(t).replace(/(.)\/+$/, "$1");
  return null;
};

/** A remote file or folder as typed, split into its folder (as remoteDirArg
 *  gives it) and its name. null: not a path, or no name to fetch (`/`, `~`,
 *  a name of `.` or `..`). */
export const remoteItemArg = (typed: string): { dir: string; name: string } | null => {
  const arg = remoteDirArg(typed);
  if (arg === null || arg === "" || arg === "/") return null;
  const cut = arg.lastIndexOf("/");
  const name = arg.slice(cut + 1);
  if (!name || name === "." || name === "..") return null;
  const dir = cut < 0 ? "" : cut === 0 ? "/" : arg.slice(0, cut);
  return { dir, name };
};

export const joinRemote = (dir: string, name: string): string => (dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`);

/** The last name in a local path: "" for a root (`/`, `C:\`). A path that
 *  starts with `/` only splits at `/`, as in fs/localPath. */
export const localBase = (path: string): string => {
  const posix = path.startsWith("/");
  const trimmed = path.replace(posix ? /\/+$/ : /[\\/]+$/, "");
  const name = trimmed.split(posix ? "/" : /[\\/]/).pop() ?? "";
  return /^[a-zA-Z]:$/.test(name) ? "" : name;
};

// ── The steps ────────────────────────────────────────────────────────────────

export type Cmd = "sftp_upload_file" | "sftp_upload_dir" | "sftp_download_file" | "sftp_download_dir";

/** One transfer on one server: the command and what it's given. A folder
 *  goes to (or comes into) the folder above it, as the commands take it. */
export interface Step {
  cmd: Cmd;
  localPath: string;
  remotePath: string;
  /** The name the command reports progress under. */
  name: string;
  /** It was already there when checked. */
  there: boolean;
}

/** A file or folder on this computer, picked to send. */
export interface LocalPick {
  path: string;
  name: string;
  isDir: boolean;
}

/** What to send to one server: each pick into `remoteDir` (the folder's
 *  full path, as the server gave it), with whether it's there already. */
export const pushSteps = (picks: LocalPick[], remoteDir: string, namesThere: ReadonlySet<string>): Step[] =>
  picks.map((p) => ({
    cmd: p.isDir ? "sftp_upload_dir" : "sftp_upload_file",
    localPath: p.path,
    remotePath: p.isDir ? remoteDir : joinRemote(remoteDir, p.name),
    name: p.name,
    there: namesThere.has(p.name),
  }));

/** What to fetch from one server: `remoteDir`/`name` into `serverFolder`, the
 *  folder on this computer that's that server's own. */
export const pullStep = (remoteDir: string, name: string, isDir: boolean, serverFolder: string, there: boolean): Step => ({
  cmd: isDir ? "sftp_download_dir" : "sftp_download_file",
  localPath: isDir ? serverFolder : localJoin(serverFolder, name),
  remotePath: joinRemote(remoteDir, name),
  name,
  there,
});

/** The kind of transfer a step's progress events say they are. */
export const kindOf = (cmd: Cmd): "upload" | "download" => (cmd.startsWith("sftp_upload") ? "upload" : "download");

/** Whether a progress event is this step's: the commands name their own
 *  transfer ids, so a step knows its own by kind and name. */
export const isStepEvent = (step: Step, ev: { kind?: string; name?: string }): boolean =>
  ev.kind === kindOf(step.cmd) && ev.name === step.name;

// ── Skip or replace ──────────────────────────────────────────────────────────

/** One choice for the whole run, about what's already there. */
export type Choice = "skip" | "replace";

/** The `overwrite` a step runs with, or null to skip it. Only what was there
 *  when checked is replaced, and only when the user said so; anything that
 *  turns up since is refused by the command (EXISTS) and counted skipped. */
export const overwriteFor = (step: Step, choice: Choice): boolean | null => {
  if (!step.there) return false;
  return choice === "replace" ? true : null;
};

/** How a step that didn't finish ended, from the command's error. */
export const failureOf = (err: unknown): { end: "skipped" } | { end: "stopped" } | { end: "failed"; reason: string } => {
  const text = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
  if (text.startsWith("EXISTS:")) return { end: "skipped" };
  if (text === "cancelled") return { end: "stopped" };
  return { end: "failed", reason: text || "It failed." };
};

// ── Each server's row ────────────────────────────────────────────────────────

export type Row =
  | { state: "queued" }
  | { state: "copying"; step: number; steps: number; pct: number | null }
  | { state: "done"; copied: number; skipped: number; note?: string }
  | { state: "skipped" }
  | { state: "failed"; reason: string }
  | { state: "stopped" };

/** A server's row once its steps are over. */
export const finalRow = (copied: number, skipped: number, note?: string): Row =>
  copied === 0 && skipped > 0 ? { state: "skipped" } : { state: "done", copied, skipped, ...(note ? { note } : {}) };

export const percent = (bytes: number, total: number): number | null =>
  total > 0 ? Math.min(100, Math.floor((bytes * 100) / total)) : null;

export const rowText = (r: Row): string => {
  switch (r.state) {
    case "queued": return "Waiting";
    case "copying": {
      const of = r.steps > 1 ? ` (${r.step} of ${r.steps})` : "";
      return `Copying${r.pct === null ? "" : ` ${r.pct}%`}${of}`;
    }
    case "done": return `Done${r.skipped ? `, ${r.skipped} already there left as they were` : ""}${r.note ? `. ${r.note}` : ""}`;
    case "skipped": return "Skipped: already there";
    case "failed": return `Failed: ${r.reason}`;
    case "stopped": return "Stopped";
  }
};

/** The line under a finished run. */
export const runTotals = (rows: Row[]): string => {
  const n = (s: Row["state"]) => rows.filter((r) => r.state === s).length;
  const parts = [
    `${n("done")} done`,
    n("skipped") ? `${n("skipped")} skipped` : "",
    n("failed") ? `${n("failed")} failed` : "",
    n("stopped") + n("queued") ? `${n("stopped") + n("queued")} stopped` : "",
  ];
  return parts.filter(Boolean).join(", ") + ".";
};

// ── The summary before it starts ─────────────────────────────────────────────

/** What a server's check found: its steps, or why it can't take part. */
export type Check = { ok: true; steps: Step[] } | { ok: false; reason: string };

export interface Summary {
  /** Servers that will copy. */
  servers: number;
  /** Servers that can't, with the reason. */
  problems: number;
  /** Steps whose target is there already, and on how many servers. */
  there: number;
  thereServers: number;
}

export const summarize = (checks: Check[]): Summary => {
  const ok = checks.filter((c): c is { ok: true; steps: Step[] } => c.ok);
  const there = ok.map((c) => c.steps.filter((s) => s.there).length);
  return {
    servers: ok.length,
    problems: checks.length - ok.length,
    there: there.reduce((a, b) => a + b, 0),
    thereServers: there.filter((n) => n > 0).length,
  };
};

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

export const formatBytes = (n: number): string => {
  if (n < 1024) return plural(n, "byte");
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
};

/** What a push sends to each server, counted on this computer. `more`: the
 *  count stopped at its limit, so it's at least this. */
export interface LocalCount { files: number; bytes: number; more: boolean }

export const pushLine = (count: LocalCount, servers: number, dir: string): string =>
  `${count.more ? "At least " : ""}${plural(count.files, "file")} (${formatBytes(count.bytes)}) to ${plural(servers, "server")}, into ${dir}.`;

export const pullLine = (path: string, servers: number, folder: string): string =>
  `${path} from ${plural(servers, "server")}, each into its own folder in ${folder}.`;

export const thereLine = (s: Summary, mode: Mode): string =>
  `${plural(s.there, "of these is", "of these are")} already there${s.thereServers > 1 ? `, on ${plural(s.thereServers, "server")}` : ""}${mode === "pull" ? " on this computer" : ""}.`;

// ── Counting what a push sends ───────────────────────────────────────────────

/** One entry of a local folder listing (local_list_dir). */
export interface LocalEntry { path: string; name: string; is_dir: boolean; size: number; is_symlink?: boolean }

/** Stop counting after this many folders: the count is then "at least". */
export const COUNT_FOLDERS_MAX = 2000;

/** Files and bytes under the picks, as sftp_upload_dir walks a folder: links
 *  aren't followed or sent. `list` lists one folder. At most `foldersMax`
 *  folders are listed, `AT_ONCE` at a time; past that it's "at least". */
export const countLocal = async (
  picks: (LocalPick & { size: number })[],
  list: (path: string) => Promise<LocalEntry[]>,
  foldersMax = COUNT_FOLDERS_MAX,
): Promise<LocalCount> => {
  const count: LocalCount = { files: 0, bytes: 0, more: false };
  const folders: string[] = [];
  for (const p of picks) {
    if (p.isDir) folders.push(p.path);
    else { count.files++; count.bytes += p.size; }
  }
  let listed = 0;
  const next = async (): Promise<void> => {
    for (let dir = folders.shift(); dir !== undefined; dir = folders.shift()) {
      if (listed >= foldersMax) { count.more = true; folders.length = 0; return; }
      listed++;
      const entries = await list(dir).catch(() => [] as LocalEntry[]);
      for (const e of entries) {
        if (e.is_symlink) continue;
        if (e.is_dir) folders.push(e.path);
        else { count.files++; count.bytes += e.size; }
      }
    }
  };
  // Workers take folders as they're found, until none is left for anyone.
  while (folders.length) await Promise.all(Array.from({ length: Math.min(AT_ONCE, folders.length) }, next));
  return count;
};

// ── A few at a time ──────────────────────────────────────────────────────────

/** Runs `work` on each item, at most `limit` at once, in order. Once
 *  `stopped()` says so, nothing more starts; what has started finishes on
 *  its own (its own cancel stops it). A failure in one doesn't stop the rest:
 *  `work` is expected to catch its own, and one it throws is dropped. */
export const runFew = async <T>(items: T[], limit: number, work: (item: T, index: number) => Promise<void>, stopped: () => boolean = () => false): Promise<void> => {
  let at = 0;
  const lane = async () => {
    while (at < items.length && !stopped()) {
      const i = at++;
      await work(items[i], i).catch(() => {});
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, lane));
};

/** Calls `flush` at most once every `ms`, after the first poke since the last
 *  call: progress events come many times a second per transfer, and the page
 *  redraws only this often. Nothing runs while nothing pokes. */
export const throttle = (
  flush: () => void,
  ms: number,
  later: (f: () => void, ms: number) => unknown = (f, t) => setTimeout(f, t),
): (() => void) => {
  let waiting = false;
  return () => {
    if (waiting) return;
    waiting = true;
    later(() => { waiting = false; flush(); }, ms);
  };
};

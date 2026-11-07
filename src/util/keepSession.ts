// Sessions that survive a reconnect (#126): this device's choices, and the
// words for the sessions list. The work is in src-tauri/src/keep_session.rs;
// the page's parts in components/KeptSessions.tsx.
//
// All of it is this device's, never synced: a change on another computer
// never makes this one start running things on a server.

// The setting for every server on this computer: "1" when on. Off unless set.
const DEFAULT_KEY = "submarine-keep-session-default";
// A server's own choice, by its sync id: "always" or "never" (no entry: the
// default).
const SERVER_KEY = "submarine-keep-session";
// The one-time word that a session keeps running has been shown.
const SEEN_KEY = "submarine-keep-session-seen";

/** A choice changed: every tab reads its own again. */
export const KEEP_CHANGED = "submarine-keep-session-changed";
/** Show a server's sessions list (detail: ShowSessions). The first tab of
 *  that server to hear it sets `claimed` and shows it. */
export const SHOW_SESSIONS = "submarine-kept-sessions-show";
/** From the backend: whether a terminal turned out kept (detail: KeptNews). */
export const KEPT_EVENT = "keep-session";

export type ShowSessions = { uuid: string; claimed: boolean };
export type KeptNews = { terminalId: string; kept: boolean };
export type KeepOverride = "default" | "always" | "never";

/** Whether a server's terminals keep their session: its own choice, else
 *  the default. */
export function resolveKeep(defaultOn: boolean, override: KeepOverride): boolean {
  return override === "always" || (override === "default" && defaultOn);
}

const read = (key: string): string | null => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const write = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Best-effort, like the other per-device choices.
  }
};
const changed = () => {
  if (typeof window !== "undefined") window.dispatchEvent(new Event(KEEP_CHANGED));
};

export function readKeepDefault(): boolean {
  return read(DEFAULT_KEY) === "1";
}

export function writeKeepDefault(on: boolean) {
  write(DEFAULT_KEY, on ? "1" : "0");
  changed();
}

const readOverrides = (): Record<string, unknown> => {
  try {
    const all = JSON.parse(read(SERVER_KEY) || "{}");
    return all && typeof all === "object" && !Array.isArray(all) ? all : {};
  } catch {
    return {};
  }
};

export function readKeepOverride(uuid?: string | null): KeepOverride {
  if (!uuid) return "default";
  const v = readOverrides()[uuid];
  return v === "always" || v === "never" ? v : "default";
}

export function writeKeepOverride(uuid: string, choice: KeepOverride) {
  const all = readOverrides();
  if (choice === "default") delete all[uuid];
  else all[uuid] = choice;
  write(SERVER_KEY, JSON.stringify(all));
  changed();
}

/** Whether this server's terminals keep their session, on this device. A
 *  server with no sync id (a quick connect) never does. */
export function keepsSessions(uuid?: string | null): boolean {
  return !!uuid && resolveKeep(readKeepDefault(), readKeepOverride(uuid));
}

/** True the first time ever (on this device) a terminal is kept: when to
 *  say what that means. */
export function firstKept(): boolean {
  if (read(SEEN_KEY)) return false;
  write(SEEN_KEY, "1");
  // Only once it's stored: where nothing can be, it isn't said every time.
  return read(SEEN_KEY) === "1";
}

export type KeptTool = "tmux" | "screen";

/** A kept session the server listed (keep_session.rs, SessionRow). */
export type KeptSession = {
  name: string;
  pid: number | null;
  created: number | null;
  attached: boolean;
  /** This server's, not another saved server's on the same machine. */
  ours: boolean;
  /** Its slot, 1 to 16. */
  terminal: number;
};

export type KeptSessionList = { tool: KeptTool | null; sessions: KeptSession[] };

/** What a session is called in the list. */
export function keptSessionLabel(s: KeptSession): string {
  return s.ours ? `Session ${s.terminal}` : `${s.name} (another saved server)`;
}

/** Whether it's open, and when it started where the server says. */
export function keptSessionDetail(s: KeptSession, started: (d: Date) => string = (d) => d.toLocaleString()): string {
  const open = s.attached ? "Open now" : "Waiting";
  return s.created !== null && s.created > 0 ? `${open} · started ${started(new Date(s.created * 1000))}` : open;
}

/** The list sorted for showing: this server's first, by slot. */
export function sortKeptSessions(list: KeptSession[]): KeptSession[] {
  return [...list].sort((a, b) => Number(b.ours) - Number(a.ours) || a.terminal - b.terminal || a.name.localeCompare(b.name));
}

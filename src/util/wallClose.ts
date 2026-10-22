import { invoke } from "@tauri-apps/api/core";

// What closing a Wall does with the terminals Broadcast took from the tab
// strip (Settings › Walls, or "Remember my answer" in the question):
//  - "ask": ask each time (the default);
//  - "back": put them back as tabs;
//  - "close": close them all, but still ask when one of them is busy.
// Kept on this computer (localStorage), like the other per-device settings;
// it doesn't sync.
export type WallCloseTaken = "ask" | "back" | "close";

export const WALL_CLOSE_TAKEN_KEY = "submarine-wall-close-taken";

export const readWallCloseTaken = (): WallCloseTaken => {
  try {
    const v = localStorage.getItem(WALL_CLOSE_TAKEN_KEY);
    return v === "back" || v === "close" ? v : "ask";
  } catch {
    return "ask";
  }
};

export const writeWallCloseTaken = (v: WallCloseTaken) => {
  try { localStorage.setItem(WALL_CLOSE_TAKEN_KEY, v); } catch { /* asks each time */ }
};

// What closing these sessions would cut short, said in a sentence, or null
// when none of them is busy:
//  - file transfers running on them (uploads, downloads, a long delete);
//  - archive and extract jobs running on the server;
//  - mirrors running (closing the session stops them);
//  - Live Edit files with changes not sent to the server yet;
//  - AI commands running there, or waiting for you to allow them.
// The first four come from the app's own record (busy.rs); the AI ones from
// the AI panel's state, passed in.
interface SessionBusy { session_id: string; transfers: number; archives: number; mirrors: number; unsent_edits: number }

const count = (n: number, one: string, many: string) => `${n === 1 ? "a" : n} ${n === 1 ? one : many}`;

export const describeBusy = async (
  sessionIds: string[],
  aiCommands: { session_id: string }[],
): Promise<string | null> => {
  if (sessionIds.length === 0) return null;
  const ids = new Set(sessionIds);
  // If the app can't say, that's said instead, so "Close them all" asks
  // rather than closing what might be busy.
  let busy: SessionBusy[];
  try {
    busy = await invoke<SessionBusy[]>("sessions_busy", { sessionIds });
  } catch {
    return "Submarine couldn't check whether any of them is busy.";
  }
  const transfers = busy.reduce((n, b) => n + b.transfers, 0);
  const archives = busy.reduce((n, b) => n + b.archives, 0);
  const mirrors = busy.reduce((n, b) => n + b.mirrors, 0);
  const edits = busy.reduce((n, b) => n + b.unsent_edits, 0);
  const ai = aiCommands.filter((c) => ids.has(c.session_id));
  const servers = new Set([...busy.map((b) => b.session_id), ...ai.map((c) => c.session_id)]).size;
  if (servers === 0) return null;
  const parts = [];
  if (transfers > 0) parts.push(`${count(transfers, "file transfer", "file transfers")} running`);
  if (archives > 0) parts.push(`${count(archives, "archive", "archives")} being made or unpacked on the server`);
  if (mirrors > 0) parts.push(`${count(mirrors, "mirror", "mirrors")} running`);
  if (edits > 0) parts.push(`${count(edits, "Live Edit file", "Live Edit files")} with changes not sent yet`);
  if (ai.length > 0) parts.push(`${count(ai.length, "AI command", "AI commands")} running or waiting for you`);
  const list = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0];
  return `${servers === 1 ? "One of these servers is" : `${servers} of these servers are`} busy: ${list}.`;
};

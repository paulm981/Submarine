// "Open terminal here" (#72): from a folder in SFTP to a terminal in it.
//
// - A folder in the server pane: a new terminal on that server, which types
//   `cd '<folder>'` once its shell is ready, as on-connect commands wait for
//   the shell (useOnConnect, through withCdHere).
// - A folder in the local pane: a Home terminal that starts there, the
//   folder passed to local_terminal_open, which checks it is one.
//
// A folder's name comes from the server, so what's typed is quoted, and the
// entry isn't offered for a name a shell could read as more than a name:
// a control character (it could press keys), `!` (tcsh expands it even
// inside single quotes), or `\` (fish reads it inside them). Nor for a
// Windows server's folder (`/C:/…`), whose shells take other commands.
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { TerminalSquare } from "lucide-react";
import { IS_ANDROID, IS_MOBILE } from "../../util/platform";

/** A server tab is asked for a terminal in a folder: `{ sessionId, path }`. */
export const TERMINAL_HERE = "submarine-terminal-here";
/** Home is asked for a terminal in a folder on this computer: `{ path }`. */
export const HOME_HERE = "submarine-home-terminal-here";

/** The longest folder path offered, in characters. */
const MAX_PATH = 4096;

/** What's typed to go to a server's folder, or null when it isn't offered:
 *  not an absolute path, a Windows one, too long, or with a character a
 *  shell could read as more than a name. */
export const cdLine = (path: string): string | null => {
  if (!path.startsWith("/") || /^\/[A-Za-z]:/.test(path) || path.length > MAX_PATH) return null;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f!\\]/.test(path)) return null;
  return `cd '${path.replace(/'/g, `'\\''`)}'`;
};

// Terminals waiting for their shell, and the line to type once it's ready.
const cdOnReady = new Map<string, string>();

/** useOnConnect's callback for a terminal, with a waiting `cd` typed after
 *  whatever it types itself. Once: a terminal mounting again doesn't type
 *  it twice. */
export const withCdHere = (terminalId: string, ready: (() => void) | undefined): (() => void) | undefined => {
  if (!cdOnReady.has(terminalId)) return ready;
  return () => {
    ready?.();
    const line = cdOnReady.get(terminalId);
    cdOnReady.delete(terminalId);
    if (!line) return;
    const data = Array.from(new TextEncoder().encode(`${line}\n`));
    invoke("write_terminal_data", { terminalId, data }).catch(() => {});
  };
};

/** The SFTP menu's entry, for one folder: in the server pane (`sessionId`),
 *  or the local one (none), where Home terminals run. */
export const TerminalHereItem = ({ path, sessionId, done }: { path: string; sessionId?: string; done: () => void }) => {
  const remote = sessionId !== undefined;
  if (IS_MOBILE || (remote ? cdLine(path) === null : IS_ANDROID)) return null;
  const open = () => {
    done();
    window.dispatchEvent(new CustomEvent(remote ? TERMINAL_HERE : HOME_HERE, { detail: { sessionId, path } }));
  };
  return (
    <button onClick={open} className="w-full flex items-center gap-2 p-1.5 rounded hover:bg-white/10 text-left hover:text-white">
      <TerminalSquare size={11} className="text-indigo-400" /><span>Open terminal here</span>
    </button>
  );
};

/** In a server tab: a terminal for each folder asked for there (TERMINAL_HERE),
 *  opened by `openTerminal` (a new terminal with that id, in front). */
export const TerminalHereListener = ({ sessionId, openTerminal }: { sessionId: string; openTerminal: (id: string) => void }) => {
  const opener = useRef(openTerminal);
  opener.current = openTerminal;
  useEffect(() => {
    const here = (e: Event) => {
      const d = (e as CustomEvent<{ sessionId?: string; path?: string }>).detail;
      if (d?.sessionId !== sessionId || typeof d.path !== "string") return;
      const line = cdLine(d.path);
      if (!line) return;
      const id = `${sessionId}-term-${Date.now()}`;
      cdOnReady.set(id, line);
      opener.current(id);
    };
    window.addEventListener(TERMINAL_HERE, here);
    return () => window.removeEventListener(TERMINAL_HERE, here);
  }, [sessionId]);
  return null;
};

/** Home: a folder asked for (HOME_HERE), for `open` (a new terminal in it). */
export const useHomeHere = (open: (cwd: string) => void) => {
  const opener = useRef(open);
  opener.current = open;
  useEffect(() => {
    const here = (e: Event) => {
      const path = (e as CustomEvent<{ path?: string }>).detail?.path;
      if (typeof path === "string" && path.length > 0 && path.length <= MAX_PATH) opener.current(path);
    };
    window.addEventListener(HOME_HERE, here);
    return () => window.removeEventListener(HOME_HERE, here);
  }, []);
};

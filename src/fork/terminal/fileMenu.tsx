// Right-click on a file name in a terminal (#40): Paste, Copy path, Edit
// locally.
//
// - **Only on a name that looks like a path** (`pathLike`): one with a `/`,
//   or a name with an extension (`nginx.conf`, `app.log:12`), in any
//   alphabet. Not a prompt (`paul@box:~/app$`), a remote path
//   (`user@host:/etc/hosts`) or a web address. Anywhere else, and with text
//   selected, right-click pastes as it always did. Paste is the menu's first
//   entry, so it's still one click away.
// - **The word is read from the terminal's cells** (`wordUnder`): a wide
//   character (CJK, emoji) takes two cells and one character, and a long
//   path wrapped onto the next row is one word.
// - **A relative name is resolved** against the folder the shell said it's
//   in (OSC 7 or iTerm2's OSC 1337, the same reports TerminalView reads for
//   SFTP's Follow); read here too, after TerminalView's own reader, which
//   still gets them. Without one, only an absolute path can be edited.
// - **Only on the tab's own server.** Once the shell has reported another
//   host (an ssh onward), nothing is edited, absolute paths included, until
//   it reports its own again. iTerm2's folder reports carry no host: its
//   `RemoteHost` report gives it, and without one a host-less report
//   after a hosted one can't be told apart, so it counts as away. The menu
//   says which server it edits on, for an onward ssh that reports nothing.
// - **Edit locally** is Live Edit (`sftp_open_remote_file`), which never runs
//   what it opens. Not offered in a Home terminal, a container's (its paths
//   are the container's, not the server's), for a folder (`logs/`), or for a
//   name that isn't a plain path: a control character, `..`, or longer than
//   MAX_PATH.
// - It edits as the server tab's login: a file only root can read, after
//   `sudo -i` in the terminal, fails to open, and the menu says so.
import { invoke } from "@tauri-apps/api/core";
import type { IBuffer, Terminal } from "@xterm/xterm";
import { createRoot, type Root } from "react-dom/client";
import { useEffect, useState } from "react";
import { ClipboardPaste, Copy, FileEdit } from "lucide-react";
import { nextShellFolder, parseOsc1337, parseOsc7, type ShellFolder, type ShellFolderState } from "../../util/shellFolder";

/** The longest path taken, in characters. */
const MAX_PATH = 4096;
/** The most rows one wrapped line is read across. */
const MAX_ROWS = 64;
/** A control character, C0 or C1. */
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

// The folder each terminal's shell last said it's in, by terminal id, kept
// as SFTP's Follow keeps it: a report from another host (an ssh onward from
// that terminal) marks it away, and a name then can't be told for sure.
const folders = new Map<string, ShellFolderState>();
// The host iTerm2's shell integration last named (`RemoteHost=user@host`).
const remoteHosts = new Map<string, string>();
// Terminals running a shell in a container (open_container_terminal).
const containers = new Set<string>();

/** A container's shell runs in this terminal: its paths aren't the server's. */
export const markContainer = (terminalId: string) => containers.add(terminalId);

/** The state after a folder report, or null when nothing changed. As
 *  nextShellFolder, except that a report whose host can't be compared with
 *  the first one's (one has a host, the other none) counts as away: a host
 *  never becomes "home" after host-less reports, and a host-less report
 *  after hosted ones isn't taken for home. Exported for the tests. */
export const nextFolder = (prev: ShellFolderState | undefined, f: ShellFolder): ShellFolderState | null => {
  const host = f.host.toLowerCase() === "localhost" ? "" : f.host.toLowerCase();
  if (prev && !prev.home !== !host) {
    const away = host || "?";
    return prev.away === away ? null : { ...prev, away };
  }
  return nextShellFolder(prev, f.path, f.host);
};

/** Read the folder reports a shell sends, after TerminalView's reader (which
 *  is tried first, as the last registered is, and still gets them). */
export const trackFolder = (term: Terminal, terminalId: string) => {
  const note = (f: ShellFolder | null) => {
    const next = f ? nextFolder(folders.get(terminalId), f) : null;
    if (next) folders.set(terminalId, next);
    return false;
  };
  term.parser.registerOscHandler(7, (data) => note(parseOsc7(data)));
  term.parser.registerOscHandler(1337, (data) => {
    const remote = /^RemoteHost=(?:[^@]*@)?([^\s\u0000-\u001f\u007f]{1,255})$/.exec(data);
    if (remote) {
      remoteHosts.set(terminalId, remote[1]);
      return false;
    }
    const f = parseOsc1337(data);
    return note(f && { ...f, host: remoteHosts.get(terminalId) ?? "" });
  });
};

/** The folder a terminal's shell is in on its own server: undefined while
 *  it hasn't said, or while it's on another host. */
export const folderOf = (terminalId: string): string | undefined => {
  const f = folders.get(terminalId);
  return f && !f.away ? f.path : undefined;
};

/** The shell has said it's on another host: nothing there is the server's. */
export const isAway = (terminalId: string): boolean => !!folders.get(terminalId)?.away;

/** The terminal has gone: what was kept for it goes too. */
export const forgetFolder = (terminalId: string) => {
  folders.delete(terminalId);
  remoteHosts.delete(terminalId);
  containers.delete(terminalId);
};

/** The word at `col` in a line of text: what spaces, quotes and brackets
 *  end, with a trailing `:line[:col]` and punctuation taken off. */
export const wordAt = (line: string, col: number): string => {
  const stop = /[\s"'`()<>[\]{}|;,]/;
  if (col < 0 || col >= line.length || stop.test(line[col])) return "";
  let start = col;
  let end = col;
  while (start > 0 && !stop.test(line[start - 1])) start--;
  while (end < line.length - 1 && !stop.test(line[end + 1])) end++;
  return line
    .slice(start, end + 1)
    .replace(/(:\d+){1,2}:?$/, "")
    .replace(/[.:!?]+$/, "");
};

/** The text of the line at row `y`, column `x` of `buf` (buffer rows, not
 *  screen rows), joined across the rows it wraps onto, and where the cell
 *  at `x` is in that text: a wide character is one character in two cells,
 *  and the second of them points at it. */
export const lineAt = (buf: IBuffer, y: number, x: number): { line: string; col: number } | null => {
  const row = buf.getLine(y);
  if (!row) return null;
  let first = y;
  while (first > 0 && y - first < MAX_ROWS && buf.getLine(first)?.isWrapped) first--;
  let line = "";
  let col = -1;
  for (let r = first; r < first + MAX_ROWS; r++) {
    const l = buf.getLine(r);
    if (!l || (r > first && !l.isWrapped)) break;
    for (let c = 0; c < l.length; c++) {
      const cell = l.getCell(c);
      if (!cell) break;
      // The second cell of a wide character: it's the one before it.
      if (cell.getWidth() === 0) {
        if (r === y && c === x) col = Math.max(0, line.length - 1);
        continue;
      }
      if (r === y && c === x) col = line.length;
      line += cell.getChars() || " ";
    }
  }
  return col < 0 ? null : { line: line.trimEnd(), col };
};

/** The word under row `y`, column `x` of the terminal's active buffer. */
export const wordUnder = (term: Terminal, y: number, x: number): string => {
  const at = lineAt(term.buffer.active, y, x);
  return at ? wordAt(at.line, at.col) : "";
};

/** Whether a word looks like a file's path or name: one with a `/` (not a
 *  web address, a prompt or another host's path), or a name with an
 *  extension that has a letter in it. */
export const pathLike = (word: string): boolean => {
  if (!word || word.length > MAX_PATH || /^[a-z][a-z0-9+.-]*:\/\//i.test(word)) return false;
  // `paul@box:~/app$`, `root@web1:/etc#`, `user@host:/etc/hosts`.
  if (/^[^\s/@]+@[^\s/:]+:/.test(word) || /[$#]$/.test(word)) return false;
  if (/^www\./i.test(word)) return false;
  if (word.includes("/")) return /[\p{L}\p{N}._~-]/u.test(word);
  // A dotfile (`.env`, `.bashrc`), or a name with an extension.
  if (/^\.\p{L}[\p{L}\p{N}_.-]*$/u.test(word)) return true;
  return /^[\p{L}\p{N}_.-]*[\p{L}\p{N}_-]\.[\p{L}\p{N}]*\p{L}[\p{L}\p{N}]*$/u.test(word) && !/^\d+(\.\d+)+$/.test(word);
};

/** The absolute path a word names on the server, from the folder its shell
 *  is in; null when it can't be told for sure (a relative name with no
 *  folder known, `~`, `..`, or a control character in it or the folder). */
export const resolvePath = (word: string, folder: string | undefined): string | null => {
  if (word.length > MAX_PATH || word.startsWith("~") || CONTROL.test(word)) return null;
  if (folder !== undefined && (folder.length > MAX_PATH || CONTROL.test(folder))) return null;
  const path = word.startsWith("/") ? word : folder ? `${folder.replace(/\/+$/, "")}/${word.replace(/^\.\//, "")}` : null;
  if (!path || path.startsWith("~") || path.length > MAX_PATH) return null;
  const parts = path.split("/").filter((p) => p !== "" && p !== ".");
  if (parts.includes("..")) return null;
  return `/${parts.join("/")}`;
};

/** What Edit locally opens for `word` in a terminal: null when there's
 *  nothing it can open for sure. The shell is on another host (absolute
 *  paths too), the word names a folder (`logs/`), or the path can't be told. */
export const pathToEdit = (word: string, folder: string | undefined, away: boolean): string | null =>
  away || word.endsWith("/") ? null : resolvePath(word, folder);

/** The buffer row and column under the pointer. */
const cellAt = (term: Terminal, ev: MouseEvent): { y: number; x: number } | null => {
  const screen = term.element?.querySelector(".xterm-screen");
  if (!screen) return null;
  const r = screen.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return null;
  const x = Math.floor(((ev.clientX - r.left) / r.width) * term.cols);
  const row = Math.floor(((ev.clientY - r.top) / r.height) * term.rows);
  if (x < 0 || row < 0 || x >= term.cols || row >= term.rows) return null;
  return { y: term.buffer.active.viewportY + row, x };
};

type MenuProps = {
  x: number; y: number; word: string; path: string | null; sessionId: string | null; server: string;
  paste: () => void; close: () => void;
};

const item = "w-full flex items-center gap-2 px-2.5 py-1.5 rounded text-left text-[12px] text-zinc-200 hover:bg-white/10 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent";

const Menu = ({ x, y, word, path, sessionId, server, paste, close }: MenuProps) => {
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    const away = (e: Event) => { if (!(e.target as Element)?.closest?.("[data-file-menu]")) close(); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    window.addEventListener("mousedown", away, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", close);
    };
  }, [close]);
  const edit = async () => {
    if (!path || !sessionId) return;
    setNote("Opening it in your editor…");
    try {
      await invoke("sftp_open_remote_file", { sessionId, remotePath: path });
      close();
    } catch (e) {
      setNote(`Couldn't open it: ${String(e).replace(/^\[[A-Z_]+\]\s*/, "")}`);
    }
  };
  const left = Math.max(4, Math.min(x, window.innerWidth - 260));
  const top = Math.max(4, Math.min(y, window.innerHeight - 180));
  const why = word.endsWith("/")
    ? "It's a folder"
    : "Its folder isn't known: the shell hasn't said where it is, or it's on another host";
  return (
    <div data-file-menu role="menu" style={{ left, top }}
      className="fixed z-[200] w-[250px] p-1 rounded-lg border border-white/10 bg-[#15151a] shadow-2xl">
      <div className="px-2.5 pt-1 pb-1.5 text-[10.5px] font-mono text-zinc-500 truncate" title={path ?? word}>{path ?? word}</div>
      <button role="menuitem" className={item} onClick={() => { close(); paste(); }}>
        <ClipboardPaste size={12} className="text-zinc-400" /> Paste
      </button>
      <button role="menuitem" className={item} onClick={() => { void navigator.clipboard.writeText(path ?? word).catch(() => {}); close(); }}>
        <Copy size={12} className="text-zinc-400" /> Copy path
      </button>
      {sessionId && (
        <button role="menuitem" className={item} disabled={!path} onClick={() => void edit()}
          title={path ? `Opens it from ${server || "this server"} in your editor, and saves it back there when you save` : why}>
          <FileEdit size={12} className="text-zinc-400" /> Edit locally{server ? <span className="text-zinc-500 truncate">on {server}</span> : null}
        </button>
      )}
      {note && <p className="px-2.5 py-1.5 text-[11px] text-zinc-400 leading-snug break-words">{note}</p>}
    </div>
  );
};

let open: { root: Root; host: HTMLDivElement } | null = null;

const closeMenu = () => {
  const o = open;
  open = null;
  if (!o) return;
  // Unmounted after the click that closed it has finished.
  setTimeout(() => { o.root.unmount(); o.host.remove(); });
};

/** TerminalView's right-click: the menu, on a name that looks like a path,
 *  and true; anywhere else, or with text selected, false (it pastes). */
export const fileMenuAt = (
  ev: MouseEvent,
  term: Terminal,
  t: { terminalId: string; sessionId: string; local?: unknown; serverName?: string },
  paste: () => void,
): boolean => {
  if (term.hasSelection()) return false;
  const cell = cellAt(term, ev);
  if (!cell) return false;
  const word = wordUnder(term, cell.y, cell.x);
  if (!pathLike(word)) return false;
  const serverShell = !t.local && !containers.has(t.terminalId);
  const path = pathToEdit(word, folderOf(t.terminalId), isAway(t.terminalId));
  closeMenu();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  open = { root, host };
  root.render(
    <Menu x={ev.clientX} y={ev.clientY} word={word} path={path} sessionId={serverShell ? t.sessionId : null}
      server={t.serverName ?? ""} paste={paste} close={closeMenu} />,
  );
  return true;
};

// Right-click on a file name in a terminal (#40): Paste, Copy path, Edit
// locally.
//
// - **Only on a name that looks like a path** (`pathLike`): one with a `/`,
//   or a name with an extension (`nginx.conf`, `app.log:12`). Anywhere else,
//   and with text selected, right-click pastes as it always did. Paste is the
//   menu's first entry, so it's still one click away.
// - **A relative name is resolved** against the folder the shell said it's
//   in (OSC 7 or iTerm2's OSC 1337, the same reports TerminalView reads for
//   SFTP's Follow); read here too, after TerminalView's own reader, which
//   still gets them. Without one, only an absolute path can be edited.
// - **Edit locally** is Live Edit (`sftp_open_remote_file`), which never runs
//   what it opens. Not offered in a Home terminal, a container's (its paths
//   are the container's, not the server's), or for a name that isn't a
//   plain path: a control character, `..`, or longer than MAX_PATH.
// - It edits as the server tab's login: a file only root can read, after
//   `sudo -i` in the terminal, fails to open, and the menu says so.
import { invoke } from "@tauri-apps/api/core";
import type { Terminal } from "@xterm/xterm";
import { createRoot, type Root } from "react-dom/client";
import { useEffect, useState } from "react";
import { ClipboardPaste, Copy, FileEdit } from "lucide-react";
import { nextShellFolder, parseOsc1337, parseOsc7, type ShellFolder, type ShellFolderState } from "../../util/shellFolder";

/** The longest path taken, in characters. */
const MAX_PATH = 4096;

// The folder each terminal's shell last said it's in, by terminal id, kept
// as SFTP's Follow keeps it: a report from another host (an ssh onward from
// that terminal) marks it away, and a name then can't be told for sure.
const folders = new Map<string, ShellFolderState>();
// Terminals running a shell in a container (open_container_terminal).
const containers = new Set<string>();

/** A container's shell runs in this terminal: its paths aren't the server's. */
export const markContainer = (terminalId: string) => containers.add(terminalId);

/** Read the folder reports a shell sends, after TerminalView's reader (which
 *  is tried first, as the last registered is, and still gets them). */
export const trackFolder = (term: Terminal, terminalId: string) => {
  const note = (f: ShellFolder | null) => {
    const next = f ? nextShellFolder(folders.get(terminalId), f.path, f.host) : null;
    if (next) folders.set(terminalId, next);
    return false;
  };
  term.parser.registerOscHandler(7, (data) => note(parseOsc7(data)));
  term.parser.registerOscHandler(1337, (data) => note(parseOsc1337(data)));
};

/** The folder a terminal's shell is in on its own server: undefined while
 *  it hasn't said, or while it's on another host. */
export const folderOf = (terminalId: string): string | undefined => {
  const f = folders.get(terminalId);
  return f && !f.away ? f.path : undefined;
};

/** The terminal has gone: what was kept for it goes too. */
export const forgetFolder = (terminalId: string) => {
  folders.delete(terminalId);
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

/** Whether a word looks like a file's path or name: one with a `/` (not a
 *  web address), or a name with an extension that has a letter in it. */
export const pathLike = (word: string): boolean => {
  if (!word || word.length > MAX_PATH || /^[a-z][a-z0-9+.-]*:\/\//i.test(word)) return false;
  if (word.includes("/")) return /[A-Za-z0-9._~-]/.test(word);
  // A dotfile (`.env`, `.bashrc`), or a name with an extension.
  if (/^\.[A-Za-z][\w.-]*$/.test(word)) return true;
  return /^[\w.-]*[\w-]\.[A-Za-z0-9]*[A-Za-z][A-Za-z0-9]*$/.test(word) && !/^\d+(\.\d+)+$/.test(word);
};

/** The absolute path a word names on the server, from the folder its shell
 *  is in; null when it can't be told for sure (a relative name with no
 *  folder known, `~`, `..`, or a control character). */
export const resolvePath = (word: string, folder: string | undefined): string | null => {
  // eslint-disable-next-line no-control-regex
  if (word.length > MAX_PATH || word.startsWith("~") || /[\u0000-\u001f\u007f-\u009f]/.test(word)) return null;
  const path = word.startsWith("/") ? word : folder ? `${folder.replace(/\/+$/, "")}/${word.replace(/^\.\//, "")}` : null;
  if (!path || path.startsWith("~")) return null;
  const parts = path.split("/").filter((p) => p !== "" && p !== ".");
  if (parts.includes("..")) return null;
  return `/${parts.join("/")}`;
};

/** The text of the terminal's row under the pointer, and the column. */
const cellAt = (term: Terminal, ev: MouseEvent): { line: string; col: number } | null => {
  const screen = term.element?.querySelector(".xterm-screen");
  if (!screen) return null;
  const r = screen.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return null;
  const col = Math.floor(((ev.clientX - r.left) / r.width) * term.cols);
  const row = Math.floor(((ev.clientY - r.top) / r.height) * term.rows);
  if (col < 0 || row < 0 || col >= term.cols || row >= term.rows) return null;
  const line = term.buffer.active.getLine(term.buffer.active.viewportY + row)?.translateToString(true) ?? "";
  return { line, col };
};

type MenuProps = {
  x: number; y: number; word: string; path: string | null; sessionId: string | null;
  paste: () => void; close: () => void;
};

const item = "w-full flex items-center gap-2 px-2.5 py-1.5 rounded text-left text-[12px] text-zinc-200 hover:bg-white/10 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent";

const Menu = ({ x, y, word, path, sessionId, paste, close }: MenuProps) => {
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
  const top = Math.max(4, Math.min(y, window.innerHeight - 160));
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
          title={path ? "Opens it in your editor, and saves it back to the server when you save" : "Its folder isn't known: the shell hasn't said where it is"}>
          <FileEdit size={12} className="text-zinc-400" /> Edit locally
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
  t: { terminalId: string; sessionId: string; local?: unknown },
  paste: () => void,
): boolean => {
  if (term.hasSelection()) return false;
  const cell = cellAt(term, ev);
  if (!cell) return false;
  const word = wordAt(cell.line, cell.col);
  if (!pathLike(word)) return false;
  const serverShell = !t.local && !containers.has(t.terminalId);
  const path = resolvePath(word, folderOf(t.terminalId));
  closeMenu();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  open = { root, host };
  root.render(<Menu x={ev.clientX} y={ev.clientY} word={word} path={path} sessionId={serverShell ? t.sessionId : null} paste={paste} close={closeMenu} />);
  return true;
};

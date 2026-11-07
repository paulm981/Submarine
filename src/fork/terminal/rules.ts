// ─────────────────────────────────────────────────────────────────────────────
// The fork's rules for a terminal, as plain values and functions that
// TerminalView and the rest of src/fork/terminal use (#163).
// ─────────────────────────────────────────────────────────────────────────────

// The backend commands behind a terminal: an SSH shell's, or a shell on this
// computer's (src-tauri/src/local_term.rs). Both send output the same way
// (over the channel the terminal is opened with, src-tauri/src/term_output.rs),
// so only the commands differ.
export const SSH_COMMANDS = { write: 'write_terminal_data', resize: 'resize_terminal', close: 'close_terminal' };
export const LOCAL_COMMANDS = { write: 'local_terminal_write', resize: 'local_terminal_resize', close: 'local_terminal_close' };

// When a freshly opened shell counts as ready for typed-in commands (the
// per-server "run on connect" ones): once its output has paused this long —
// past the login banner, at the prompt — or when it has printed nothing for
// the first, longer while, and at the latest after the cap.
export const SHELL_QUIET_MS = 300;
export const SHELL_FIRST_OUTPUT_MS = 1500;
export const SHELL_READY_CAP_MS = 5000;

// Escape sequences xterm.js carries out once per count, with no limit on the
// count: scroll up/down (S, T), insert/delete lines (L, M), tab forward/back
// (I, Z), and repeat the last character (b). `ESC[999999999S` — 15 bytes,
// which a binary file `cat` to the screen can contain by chance — kept the
// window busy for hours, and thousands of `ESC[9999S` in one batch for
// minutes. Programs send counts up to the screen's size, so:
//   - a count over a few screens (countLimit) is ignored, and
//   - what one batch of output may cost is capped, about 0.3 s of work each:
//     LINE_BUDGET lines moved (S, T, L, M), CHAR_BUDGET characters repeated
//     or tab stops passed (b, I, Z). Past that, the rest of the batch's
//     counted sequences are ignored.
// A repeat copies the whole cell before the cursor, combining marks and
// all, so it costs the count times that cell's length; one of a cell with
// more than a character and a mark in it is ignored.
export const COUNTED_FINALS = ['S', 'T', 'L', 'M', 'I', 'Z', 'b'] as const;
export const LINE_BUDGET = 100_000;
export const CHAR_BUDGET = 1_000_000;
export const LINE_FINALS = new Set<string>(['S', 'T', 'L', 'M']);

// A container's terminal that needed root, and the password wasn't given.
export const ROOT_CANCELLED = '\x1b[33mThis container\'s terminal needs root on the server. Close this tab and open it again to enter the password.\x1b[0m';

// The user a password prompt asks about, when it names one, so a saved
// password is only offered for that user (#182 in the original project):
// "[sudo] password for bob:", "[dzdo] password for bob:", "bob@web1's
// password:", "Password for bob@CORP.EXAMPLE:". Null when it names nobody,
// as su's "Password:" doesn't.
export const passwordPromptUser = (out: string): string | null => {
  const clean = out
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "")
    .replace(/[\r\n]+/g, "\n");
  const line = clean.slice(-160).trimEnd().split("\n").pop() ?? "";
  const m = /password for ([^\s:]+)\s*:\s*$/i.exec(line)
    ?? /([^\s:']+)'s password\s*:\s*$/i.exec(line);
  return m ? sameUser(m[1]) : null;
};
// A user name without its host or domain ("bob@web1", "CORP\\bob" and
// "bob@corp.example" are all "bob"), lowercased, to compare by.
export const sameUser = (name: string): string => name.replace(/^.*\\/, "").replace(/@.*$/, "").toLowerCase();

/** A saved password the terminal's "Saved password" button offers. */
export type SavedPassword = { label: string; detail: string; serverId?: number; credentialId?: number };
/** What get_servers and get_credentials give, as far as the menu reads it. */
type ServerRow = { id: number; name: string; username?: string | null; has_password?: boolean; credential_id?: number | null };
type LoginRow = { id: number; name: string; username: string; has_password?: boolean };

// Which saved passwords are offered (the fork's rule, FORK.md): this
// server's own password and the saved login it uses, when the prompt asks
// for their user or names nobody (su); another saved login only when the
// prompt names its user. So the text of a prompt can't get the menu to
// offer a login's password the prompt has nothing to do with; the user
// still picks what's typed. The backend also types a server's own password
// only into that server's own terminals (type_saved_password); a saved
// login's goes where the user picks it. `recentOutput` is the end of what
// the terminal printed, its prompt last.
export const savedPasswords = (servers: ServerRow[], logins: LoginRow[], serverId: number, recentOutput: string): SavedPassword[] => {
  const asked = passwordPromptUser(recentOutput);
  const server = serverId > 0 ? servers.find((s) => s.id === serverId) : undefined;
  const ownLogin = server?.credential_id != null ? logins.find((c) => c.id === server.credential_id) : undefined;
  const items: SavedPassword[] = [];
  const serverUser = server ? (server.username || ownLogin?.username || "") : "";
  if (server?.has_password && (asked === null || sameUser(serverUser) === asked)) {
    items.push({ label: "This server's password", detail: serverUser || server.name, serverId: server.id });
  }
  if (ownLogin?.has_password && (asked === null || sameUser(ownLogin.username) === asked)) {
    items.push({ label: ownLogin.name, detail: `${ownLogin.username} · this server's login`, credentialId: ownLogin.id });
  }
  if (asked !== null) {
    for (const c of logins) {
      if (c.id === ownLogin?.id || !c.has_password || sameUser(c.username) !== asked) continue;
      items.push({ label: c.name, detail: c.username, credentialId: c.id });
    }
  }
  return items;
};

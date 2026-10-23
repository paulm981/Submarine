import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon, ISearchOptions } from '@xterm/addon-search';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { WebglAddon } from '@xterm/addon-webgl';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import '@xterm/xterm/css/xterm.css';
import { KeyRound } from 'lucide-react';
import { useIsNarrow } from '../hooks/useViewport';
import { MobileKeyBar, ModifiersState, ModKey, SpecialKey } from './MobileKeyBar';
import { appHotkey, markTyped, registerTerminal, terminalStateChanged } from '../ui/terminalDock';
import { noteOutput, setLooking, watchTerminal } from '../ui/terminalActivity';
import { forgetTerminal, logLine, noteTerminalOutput, noteTerminalParsed, noteTerminalRenderer } from '../util/diagnostics';
import { useConfirm } from '../ui/confirm';
import { useRootInvoke } from '../ui/rootInvoke';
import HistorySearchOverlay from './HistorySearchOverlay';
import { DEFAULT_FONT_STACK, fontFamilyCss, readFontFamily, readFontSize } from '../util/terminalFont';
import { MIN_CONTRAST_RATIO, readBoldBright, readGpu, readMinContrast, schemeForServer } from '../ui/terminalThemes';
import { HighlightStream, matcherForServer, type Matcher } from '../ui/highlight';
import { readScrollback } from '../util/terminalScrollback';
import { withoutPasteControls } from '../util/pasteText';
import { readUseLocalShell } from '../util/terminalShell';
import { IS_ANDROID, IS_LINUX, IS_WINDOWS } from '../util/platform';
import { readClipboardText } from '../util/clipboard';
import { readUseSshAgent } from '../util/sshAgent';

// Does the recent remote OUTPUT look like a no-echo password / passphrase
// prompt? Used to skip command-history capture so typed secrets (sudo, su,
// mysql -p, ssh/gpg passphrase) aren't persisted into searchable history. At a
// no-echo prompt the output still ENDS with the prompt text because the user's
// keystrokes were never echoed back. We strip ANSI colour/OSC sequences first
// and only match when the prompt is the trailing content.
const looksLikePasswordPrompt = (out: string): boolean => {
  const clean = out
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "") // OSC …BEL/ST
    .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "")          // CSI …final
    .replace(/[\r\n]+/g, "\n");
  const tail = clean.slice(-160);
  return /(?:password|passphrase)\b[^:\n]*:\s*$/i.test(tail);
};

// The user a password prompt asks about, when it names one, so a saved
// password is only offered for that user (#182 in the original project):
// "[sudo] password for bob:", "[dzdo] password for bob:", "bob@web1's
// password:", "Password for bob@CORP.EXAMPLE:". Null when it names nobody,
// as su's "Password:" doesn't.
const passwordPromptUser = (out: string): string | null => {
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
const sameUser = (name: string): string => name.replace(/^.*\\/, "").replace(/@.*$/, "").toLowerCase();

// Shared search options — highlight all matches in yellow, the active
// match in orange. `decorations` uses the marker overlay so the highlight
// survives scrollback and repaints (the addon's built-in selection-based
// mode would clobber the user's own selection every keystroke). The
// overview-ruler fields are typed as required by @xterm/addon-search
// but we don't render an overview ruler (no `overviewRulerLane` on the
// terminal), so their values are cosmetic — mirror the match colors so
// they'd still read sensibly if a ruler were ever enabled.
const SEARCH_OPTIONS: ISearchOptions = {
  caseSensitive: false,
  decorations: {
    matchBackground: '#facc15',
    matchBorder: '#facc15',
    matchOverviewRuler: '#facc15',
    activeMatchBackground: '#f97316',
    activeMatchBorder: '#f97316',
    activeMatchColorOverviewRuler: '#f97316',
  },
};

// The backend commands behind a terminal: an SSH shell's, or a shell on this
// computer's (src-tauri/src/local_term.rs). Both send output the same way
// (over the channel the terminal is opened with, src-tauri/src/term_output.rs),
// so only the commands differ.
const SSH_COMMANDS = { write: 'write_terminal_data', resize: 'resize_terminal', close: 'close_terminal' };
const LOCAL_COMMANDS = { write: 'local_terminal_write', resize: 'local_terminal_resize', close: 'local_terminal_close' };

// When a freshly opened shell counts as ready for typed-in commands (the
// per-server "run on connect" ones): once its output has paused this long —
// past the login banner, at the prompt — or when it has printed nothing for
// the first, longer while, and at the latest after the cap.
const SHELL_QUIET_MS = 300;
const SHELL_FIRST_OUTPUT_MS = 1500;
const SHELL_READY_CAP_MS = 5000;

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
const COUNTED_FINALS = ['S', 'T', 'L', 'M', 'I', 'Z', 'b'] as const;
const LINE_BUDGET = 100_000;
const CHAR_BUDGET = 1_000_000;
const LINE_FINALS = new Set<string>(['S', 'T', 'L', 'M']);

// State machine for the modifier stickies on the mobile key bar.
// off → armed → locked → off (cycled by repeated taps).
const cycleMod = (s: "off" | "armed" | "locked"): "off" | "armed" | "locked" =>
  s === "off" ? "armed" : s === "armed" ? "locked" : "off";

// How long a terminal's size must hold before its PTY gets it. A layout
// change passes through sizes for a few frames: the narrow and the wide
// layout swap at 640 px (the terminal is 2 columns wide for a frame) and
// panels animate their width. The shell redraws its prompt for every size
// it is given, so each of those left the screen garbled and the cursor out
// of place.
const PTY_SIZE_SETTLE_MS = 150;

// A container's terminal that needed root, and the password wasn't given.
// A Home terminal on Windows runs in a pseudo-console (ConPTY), which wraps
// and redraws lines itself on a resize; xterm matches it when told so, and
// better for knowing which Windows build it is. Asked for once.
let windowsBuild: Promise<number | null> | null = null;
const readWindowsBuild = () =>
  (windowsBuild ??= invoke<number | null>('local_windows_build').catch(() => null));

const ROOT_CANCELLED = '\x1b[33mThis container\'s terminal needs root on the server. Close this tab and open it again to enter the password.\x1b[0m';

const TerminalView = ({
  sessionId,
  terminalId,
  disabled = false,
  isActive = true,
  visible,
  isActiveView = false,
  inView,
  focusSignal = 0,
  containerExec,
  connectionEpoch = 0,
  serverId = 0,
  serverName = "",
  local,
  onShellReady,
  telnet = false,
  ownLogin = false,
  quickAuth = null,
}: {
  sessionId: string;
  terminalId: string;
  disabled?: boolean;
  /// Tells us when this terminal is the visible one in its parent. Used to
  /// trigger a refit + refresh whenever we become visible — without this,
  /// xterm's internal canvas can be left holding stale glyphs from before
  /// the parent's display/opacity change and the prompt appears garbled
  /// until the user types something.
  isActive?: boolean;
  /// On screen, whether or not it's the one being typed in — a split pane
  /// beside the focused one. It refits to its pane when that resizes.
  /// Defaults to `isActive`.
  visible?: boolean;
  /// True only when this terminal's SessionView is the frontmost server
  /// tab (DesktopApp's active view). Auto-focus is gated on this so a
  /// background session — or one shown in a Wall — never steals the
  /// keyboard while the user is typing in the visible one.
  isActiveView?: boolean;
  /// This terminal's server is on screen (in front, or a tile of a split):
  /// with `visible`, you are looking at it, so it has no activity mark.
  /// Defaults to isActiveView.
  inView?: boolean;
  /// Monotonic counter the parent SessionView bumps to pull keyboard focus
  /// into the active terminal for selections that DON'T change which
  /// terminal is active (re-clicking the current tab, re-selecting this
  /// server tab, or clicking a tab while focus sat in a tool pane). A
  /// change — not the value — triggers the focus; only the active terminal
  /// reacts.
  focusSignal?: number;
  /// When set, this terminal runs `docker exec -it <container> <shell>`
  /// on the SSH host instead of the user's login shell. Used by the
  /// Docker tab in InfoPanel to open an interactive session inside a
  /// specific container without leaving the app.
  containerExec?: { container: string; useSudo: boolean };
  /// A telnet server's terminal (#179): it opens the connection its tab
  /// made (`telnet_open_terminal`) instead of a shell over SSH. The rest —
  /// keys, sizes, the saved-password button — works the same.
  telnet?: boolean;
  /// Bumped by the parent SessionView on every successful reconnect.
  /// When the value changes we re-open a fresh PTY against the new SSH
  /// handle WITHOUT disposing xterm — the user keeps their scroll-back
  /// from before the drop and can copy/paste anything they did pre-drop.
  /// Initial mount is opened by the main effect, so we only act on
  /// value CHANGES after that.
  connectionEpoch?: number;
  /// Server id + name are used to stamp command-history rows so the
  /// Ctrl+R overlay can show which server the command was run against.
  /// Quick-connect sessions have serverId=0 and just record the display
  /// name — that's fine for search, since the row is orphaned from any
  /// saved node anyway.
  serverId?: number;
  serverName?: string;
  /// A shell on this computer (the Home view) instead of one on a server:
  /// `shell` is a list_local_shells id, or null for the default; `onOpened` gets
  /// the label of the shell that started. A local terminal is never shown in
  /// a Wall or broadcast to, and keeps no command history — it isn't a
  /// server session. An AI tab's can be moved into the AI side pane.
  local?: { shell: string | null; onOpened?: (label: string) => void; onFailed?: () => void };
  /// Called each time this terminal's SSH shell has been opened (first
  /// connect and every reconnect) and has printed what it prints on login,
  /// so typed-in commands land at its prompt (SessionView's run-on-connect).
  onShellReady?: () => void;
  /// This terminal logs in on a connection of its own, as a new connection
  /// to the server would, instead of opening a channel on its tab's (issue
  /// #230): for servers that tie what a user may do to the login, such as
  /// CA Access Control's `sesu`. When that login fails it uses the tab's
  /// connection and says so.
  ownLogin?: boolean;
  /// A quick connect's login (it has no saved server), for `ownLogin`.
  quickAuth?: unknown;
}) => {
  const commands = local ? LOCAL_COMMANDS : SSH_COMMANDS;

  // What draws this terminal's output (set up with its xterm, below). Each
  // open — the first and every reconnect — passes a fresh channel to the
  // backend that hands its messages to it.
  // It's told which channel each batch came over (its id), which is what the
  // batch is acknowledged under: after a reconnect, what's still being drawn
  // from the old channel isn't counted for the new one.
  const outputRef = useRef<((data: Uint8Array, channel: number) => void) | null>(null);
  // Syntax highlighting of the output (src/ui/highlight.ts).
  const highlightRef = useRef<HighlightStream | null>(null);
  const outputChannel = () => {
    const ch: Channel<ArrayBuffer> = new Channel<ArrayBuffer>((buf) => outputRef.current?.(new Uint8Array(buf), ch.id));
    return ch;
  };

  // The wait for a freshly opened shell to be ready (SHELL_QUIET_MS): armed
  // when open_terminal returns, pushed back by each output batch. Refs only,
  // so the long-lived output listener can drive it.
  const onShellReadyRef = useRef(onShellReady);
  useEffect(() => { onShellReadyRef.current = onShellReady; }, [onShellReady]);
  const readyWatchRef = useRef<{ quiet: number; cap: number } | null>(null);
  const stopReadyWatch = () => {
    const w = readyWatchRef.current;
    if (w) { window.clearTimeout(w.quiet); window.clearTimeout(w.cap); }
    readyWatchRef.current = null;
  };
  const shellReady = () => {
    if (!readyWatchRef.current) return;
    stopReadyWatch();
    onShellReadyRef.current?.();
  };
  const watchForShellReady = () => {
    if (!onShellReadyRef.current) return;
    stopReadyWatch();
    readyWatchRef.current = {
      quiet: window.setTimeout(shellReady, SHELL_FIRST_OUTPUT_MS),
      cap: window.setTimeout(shellReady, SHELL_READY_CAP_MS),
    };
  };

  // The connection this terminal's shell runs on: its tab's, or its own
  // (`ownLogin`), `<tab>::term-<terminal>`, which the backend takes as the
  // tab's for what was typed there (a password asked for, a host key).
  const ownConn = `${sessionId}::term-${terminalId}`;
  const shellConnRef = useRef(sessionId);
  // Ends the wait of this terminal's own login while it is in progress.
  const cancelOwnRef = useRef<(() => void) | null>(null);
  // Logs in on this terminal's own connection; resolves to the session to
  // open the shell on: its own, or the tab's when that login failed. Null when
  // it was cancelled (a newer login started, or the terminal or its tab went
  // away): then there is nothing to open.
  const connectOwn = async (term: Terminal): Promise<string | null> => {
    // One login at a time: a newer one (a reconnect) replaces one still in
    // progress, and the backend drops that one's connection when it starts.
    cancelOwnRef.current?.();
    let cancelled = false;
    let settle: (r: true | string | null) => void = () => {};
    const done = new Promise<true | string | null>((resolve) => { settle = resolve; });
    const cancel = () => { cancelled = true; settle(null); };
    cancelOwnRef.current = cancel;
    const offs = await Promise.all([
      listen(`connection-success-${ownConn}`, () => settle(true)),
      listen(`connection-failed-${ownConn}`, (e: any) => settle(String(e.payload?.reason ?? 'the login failed'))),
    ]);
    const timer = setTimeout(() => settle('no answer within 60 seconds'), 60000);
    if (!cancelled) {
      invoke('initiate_connection', {
        sessionId: ownConn,
        serverId,
        customPassword: null,
        quickAuth: quickAuth ?? null,
        sessionRole: 'terminal',
        separateSessions: false,
        useSshAgent: readUseSshAgent(),
      }).catch((e) => settle(String(e)));
    }
    const result = await done;
    clearTimeout(timer);
    offs.forEach((off) => off());
    if (cancelOwnRef.current === cancel) cancelOwnRef.current = null;
    if (result === null) return null;
    if (result === true) return ownConn;
    term.writeln(`\x1b[33mThis terminal couldn't log in on its own (${result}); it uses the tab's connection.\x1b[0m`);
    invoke('disconnect_session', { sessionId: ownConn }).catch(() => {});
    return sessionId;
  };
  // Ends this terminal's own login: one in progress is cancelled (the backend
  // drops its connection if it still comes up), one that's up is closed.
  const closeOwn = () => {
    const pending = cancelOwnRef.current;
    cancelOwnRef.current = null;
    pending?.();
    if (!pending && shellConnRef.current === sessionId) return;
    invoke('disconnect_session', { sessionId: ownConn }).catch(() => {});
    shellConnRef.current = sessionId;
  };
  const terminalRef = useRef<HTMLDivElement>(null);
  // What a Wall moves into its tile (terminalDock): the div xterm draws
  // into, and over it this terminal's search box, Saved password button and
  // toast, so they go with it. The ref is for long-lived handlers, the state
  // to render the overlays into it.
  const dockElRef = useRef<HTMLDivElement | null>(null);
  const [dockEl, setDockEl] = useState<HTMLDivElement | null>(null);
  const xtermRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const openedRef = useRef(false);
  // The xterm `onData` callback is bound once and persists for the life of the
  // component. We read this ref inside the callback so the latest `disabled`
  // state is observed without having to tear down and rebuild xterm.
  const disabledRef = useRef(disabled);
  useEffect(() => {
    disabledRef.current = disabled;
    terminalStateChanged();
  }, [disabled]);
  // Mirror of `isActive` readable inside the ResizeObserver callback. The
  // observer is bound once at mount and runs for the lifetime of the
  // terminal, so a stale closure over the initial `isActive` would block
  // the gate from ever flipping. A ref is the cheapest way to expose live
  // state to that callback.
  const isActiveRef = useRef(isActive);
  useEffect(() => {
    isActiveRef.current = isActive;
  }, [isActive]);
  const visibleRef = useRef(visible ?? isActive);
  useEffect(() => {
    visibleRef.current = visible ?? isActive;
  }, [visible, isActive]);
  // While a Wall shows this terminal (src/ui/terminalDock.ts) its xterm sits
  // in the Wall's tile, on screen whatever this tab is doing — so it refits
  // to the tile even when it isn't the tab's active terminal.
  const dockedRef = useRef(false);
  // Activity mark (src/ui/terminalActivity.ts): looking = shown in a server
  // tab that is on screen, or in a Wall tile.
  const inViewRef = useRef(inView ?? isActiveView);
  const lookingNow = () => (visibleRef.current && inViewRef.current) || dockedRef.current;
  // Its shell is open (open_terminal has answered, again after a reconnect):
  // what a Wall's broadcast checks, with `disabled`, before typing into it.
  const ptyLiveRef = useRef(false);
  // Mirror of isActiveView, read inside focusTerminal's rAF / signal
  // callbacks (which would otherwise close over a stale value).
  const isActiveViewRef = useRef(isActiveView);
  useEffect(() => { isActiveViewRef.current = isActiveView; }, [isActiveView]);
  useEffect(() => {
    inViewRef.current = inView ?? isActiveView;
    setLooking(terminalId, lookingNow());
  }, [terminalId, visible, isActive, isActiveView, inView]);
  // Mirror of containerExec for the reconnect effect — same reason as
  // isActiveRef. The reconnect effect is keyed on `connectionEpoch`
  // alone (the parent re-creates the containerExec object every render
  // so including it in deps would re-fire every paint).
  const containerExecRef = useRef(containerExec);
  useEffect(() => { containerExecRef.current = containerExec; }, [containerExec]);
  // Last connectionEpoch we acted on. Initial mount's PTY is opened by
  // the main effect, so we should NOT re-open on the first run of the
  // reconnect effect — only on subsequent value changes.
  const lastEpochRef = useRef(connectionEpoch);

  // Mobile-only sticky modifiers (Ctrl / Alt / Shift) for the on-screen
  // key bar. State doubles as render input (highlighting) and is mirrored
  // into a ref so the long-lived `term.onData` callback can read it without
  // re-binding every state change. Armed modifiers consume themselves on
  // the next typed char; locked modifiers persist until the user taps the
  // chip a third time.
  const isMobile = useIsNarrow();
  const [modifiers, setModifiers] = useState<ModifiersState>({ ctrl: "off", alt: "off", shift: "off" });
  const modifiersRef = useRef<ModifiersState>(modifiers);
  useEffect(() => { modifiersRef.current = modifiers; }, [modifiers]);

  // In-terminal search (Ctrl+F / Cmd+F). Query state is component-scoped so
  // reopening the chip pre-fills the last search — matches VS Code / Chrome
  // muscle memory. showSearch drives the floating chip's mount so xterm
  // isn't burdened with a hidden overlay when the user isn't searching.
  const [showSearch, setShowSearch] = useState(false);
  const [query, setQuery] = useState('');

  // Command-history overlay (Ctrl+R / Cmd+R). Same document-level keydown
  // gate as search: only pop it when the user is focused inside this
  // terminal's DOM subtree, so multi-tab windows don't fire N overlays.
  const [showHistory, setShowHistory] = useState(false);

  // Buffer keystrokes locally so we can log "the command line the user
  // typed" on each Enter press. Best-effort: pastes, arrow-key edits,
  // multi-line entries will produce noisy or wrong rows — we accept that
  // rather than reimplement a shell parser in the browser. `commandBufRef`
  // is mutated inside the long-lived onData closure so a ref (not state)
  // is the right container.
  const commandBufRef = useRef<string>("");
  // Pasted text isn't recorded as history (see onData). `pastingRef` is set
  // while our right-click paste runs — term.paste() fires onData
  // synchronously — and `commandBufPastedRef` marks a command line that still
  // holds pasted text, so its Enter is skipped too.
  const pastingRef = useRef(false);
  const commandBufPastedRef = useRef(false);
  // The app's confirm dialog, for the terminal's long-lived handlers (links,
  // multi-line paste), which would otherwise see a stale function.
  const confirm = useConfirm();
  // A container's terminal may need root (open_container_terminal).
  const rootInvoke = useRootInvoke();
  const confirmRef = useRef(confirm);
  confirmRef.current = confirm;
  // The terminal's input handler (what xterm's onData runs), for keys that
  // don't come from xterm itself — the mobile key bar's arrows and symbols.
  const inputRef = useRef<((data: string) => void) | null>(null);
  // Small tail of recent remote OUTPUT, kept only to detect no-echo password
  // prompts (see looksLikePasswordPrompt) — never persisted or displayed.
  const recentOutputRef = useRef<string>("");

  const toggleModifier = (m: ModKey) => {
    setModifiers(prev => ({ ...prev, [m]: cycleMod(prev[m]) }));
  };
  // Called after a transformed char is sent so armed modifiers reset back
  // to "off" while locked ones survive (matches Termux's behavior). Skipped
  // when nothing was actually armed — avoids a useless setState ping.
  const consumeArmedModifiers = () => {
    setModifiers(prev => {
      if (prev.ctrl !== "armed" && prev.alt !== "armed" && prev.shift !== "armed") return prev;
      return {
        ctrl:  prev.ctrl  === "armed" ? "off" : prev.ctrl,
        alt:   prev.alt   === "armed" ? "off" : prev.alt,
        shift: prev.shift === "armed" ? "off" : prev.shift,
      };
    });
  };

  // Esc / Tab from the bar bypass the onData modifier pipeline (those keys
  // produce escape sequences directly, not printable chars), but Shift+Tab
  // still has a meaningful encoding so we honor it here.
  //
  // Arrows and symbols go through the terminal's input handler like typed
  // keys. An arrow sends what xterm sends for the real key: `ESC [ A` (or
  // `ESC O A` once the program asked for application cursor keys, as vim and
  // less do), and with modifiers armed `ESC [ 1 ; <m> A`, where m is 1 plus
  // Shift 1, Alt 2, Ctrl 4. The symbols (| ~ / -) are typed characters, so the
  // handler applies the armed modifiers to them itself.
  const sendSpecialKey = (key: SpecialKey) => {
    if (disabledRef.current) return;
    if (key === "|" || key === "~" || key === "/" || key === "-") {
      inputRef.current?.(key);
      return;
    }
    if (key !== "esc" && key !== "tab") {
      const final = { up: "A", down: "B", right: "C", left: "D" }[key];
      const m = modifiersRef.current;
      const mod = 1 + (m.shift !== "off" ? 1 : 0) + (m.alt !== "off" ? 2 : 0) + (m.ctrl !== "off" ? 4 : 0);
      const appCursor = xtermRef.current?.modes.applicationCursorKeysMode ?? false;
      inputRef.current?.(mod > 1 ? `\x1b[1;${mod}${final}` : `\x1b${appCursor ? "O" : "["}${final}`);
      if (mod > 1) consumeArmedModifiers();
      return;
    }
    let bytes: number[];
    let consumes = false;
    if (key === "esc") {
      bytes = [0x1b];
    } else {
      if (modifiersRef.current.shift !== "off") {
        // ESC [ Z = CSI Z = back-tab (the standard Shift+Tab sequence).
        bytes = [0x1b, 0x5b, 0x5a];
        consumes = true;
      } else {
        bytes = [0x09];
      }
    }
    markTyped(terminalId);
    invoke(commands.write, { terminalId, data: bytes }).catch(console.error);
    if (consumes) consumeArmedModifiers();
  };

  // Reconnect handler: when the parent bumps connectionEpoch we re-open
  // the PTY against the new SSH handle and write a divider line into the
  // existing xterm so the user can scroll up to see everything they did
  // before the disconnect. The backend's `terminal_txs` HashMap.insert
  // overwrites the dead entry transparently, so write_terminal_data
  // routes to the new task without changing the terminal_id.
  useEffect(() => {
    if (connectionEpoch === lastEpochRef.current || local) return;
    lastEpochRef.current = connectionEpoch;
    const term = xtermRef.current;
    if (!term) return;
    // A new shell: nothing the old one set (a full-screen program, colors)
    // carries over into highlighting.
    const held = highlightRef.current?.reset();
    if (held) term.write(held);
    // Visible separator. \x1b[33m = yellow, \x1b[0m = reset.
    term.write('\r\n\x1b[33m── reconnected ──\x1b[0m\r\n');
    ptyLiveRef.current = false;
    terminalStateChanged();
    const opened = () => { markTyped(terminalId); ptyLiveRef.current = true; terminalStateChanged(); };
    // As on the first open: the backend only takes resizes once the shell is
    // up, so a resize that landed while it was starting again (the window
    // was being dragged, a panel opened) is sent once more when it is. Without
    // this the shell kept the size it was reopened with, and wrapped its lines
    // at the wrong column until the next resize.
    const syncSize = () => {
      invoke('resize_terminal', { terminalId, cols: term.cols, rows: term.rows }).catch(() => {});
    };
    const ce = containerExecRef.current;
    if (telnet) {
      invoke('telnet_open_terminal', {
        sessionId,
        terminalId,
        cols: term.cols || 80,
        rows: term.rows || 24,
        onOutput: outputChannel(),
      }).then(() => { syncSize(); opened(); }, e => {
        term.writeln(`\x1b[31mReconnect failed: ${e}\x1b[0m`);
      });
    } else if (ce) {
      // Through root, asking for the password when sudo or dzdo wants one.
      rootInvoke<void>('open_container_terminal', {
        sessionId,
        terminalId,
        container: ce.container,
        cols: term.cols || 80,
        rows: term.rows || 24,
        useSudo: ce.useSudo,
        onOutput: outputChannel(),
      }, 'Open the container terminal').then((r) => {
        if (r === null) { term.writeln(ROOT_CANCELLED); return; }
        syncSize(); opened();
      }, e => {
        term.writeln(`\x1b[31mReconnect into container failed: ${e instanceof Error ? e.message : e}\x1b[0m`);
      });
    } else {
      // A terminal with a login of its own logs in again first.
      (ownLogin ? connectOwn(term) : Promise.resolve(sessionId))
        .then((sid) => {
          if (sid === null) return false;
          shellConnRef.current = sid;
          return invoke('open_terminal', {
            sessionId: sid,
            terminalId,
            cols: term.cols || 80,
            rows: term.rows || 24,
            useLocalShell: readUseLocalShell(),
            onOutput: outputChannel(),
          }).then(() => true);
        })
        .then((ok) => { if (ok) { syncSize(); opened(); watchForShellReady(); } }, e => {
          term.writeln(`\x1b[31mReconnect failed: ${e}\x1b[0m`);
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionEpoch]);

  // A terminal with a login of its own: the tab disconnected, so its own
  // connection goes too (a reconnect logs it in again); and that connection
  // dropping on its own is said in the terminal.
  useEffect(() => {
    if (disabled) closeOwn();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [disabled]);
  useEffect(() => {
    if (!ownLogin) return;
    const off = listen(`session-disconnected-${ownConn}`, (e: any) => {
      // This side closed it (the terminal, its tab, a failed login): no news.
      if (e.payload?.user_initiated) return;
      shellConnRef.current = sessionId;
      xtermRef.current?.write("\r\n\x1b[33m── this terminal's own login closed; reconnect the tab to log in again ──\x1b[0m\r\n");
    });
    return () => { off.then((f) => f()); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ownConn, ownLogin]);

  // Move keyboard focus into this terminal's xterm. Shared by the
  // become-active effect and the parent-driven focus signal below, so the
  // user can type the moment a terminal tab (or its server tab) is selected
  // — issue #51. Bails in every situation where grabbing focus is wrong:
  //   • touch devices — focusing xterm pops the soft keyboard unasked;
  //   • a terminal whose SessionView isn't the frontmost server tab, or one
  //     a Wall is showing (isActiveView=false) — never steal from the
  //     session the user is actually looking at;
  //   • a disconnected / not-yet-connected terminal (dead PTY; the reconnect
  //     banner or the connect-log view owns the screen). This also covers
  //     the fingerprint / 2FA prompts, which only appear while disabled;
  //   • the user is typing in a real input (SFTP path box, settings, the
  //     auth-retry password, the in-terminal Find box). xterm's own hidden
  //     <textarea> lives inside `.xterm`, so moving focus BETWEEN terminals
  //     is still allowed;
  //   • a modal dialog is open — confirm / overwrite / text-prompt all carry
  //     aria-modal.
  const focusTerminal = () => {
    const coarsePointer =
      typeof window !== "undefined" && !!window.matchMedia?.("(pointer: coarse)")?.matches;
    if (IS_ANDROID || coarsePointer) return;
    if (!isActiveViewRef.current) return;
    if (disabledRef.current) return;
    const term = xtermRef.current;
    if (!term) return;
    const active = document.activeElement as HTMLElement | null;
    if (active) {
      const tag = active.tagName;
      const isField =
        tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || active.isContentEditable;
      if (isField && !active.closest(".xterm")) return;
    }
    if (document.querySelector('[aria-modal="true"]')) return;
    term.focus();
  };

  // Repaint when this terminal becomes the active one. The parent uses
  // opacity (within a session) or display:none (across sessions/tabs) to
  // swap visible terminals — neither triggers xterm's internal redraw, so
  // the canvas can end up showing a stale row of glyphs from before the
  // switch. Calling `fit()` recomputes cols/rows and `refresh()` forces
  // every visible row to repaint. rAF defers until the layout has settled
  // — without it `fit()` would measure 0×0 in the display:none case.
  useEffect(() => {
    if (!isActive) return;
    const id = requestAnimationFrame(() => {
      try {
        fitAddonRef.current?.fit();
        const t = xtermRef.current;
        if (t) t.refresh(0, Math.max(0, t.rows - 1));
      } catch { /* terminal not ready yet — next tick will catch it */ }
      // Layout has settled and this is now the visible terminal: land the
      // caret in it so the user can type without a second click (#51).
      focusTerminal();
    });
    return () => cancelAnimationFrame(id);
  }, [isActive]);

  // Same repaint for a pane that came on screen without the keyboard (a
  // split pane beside the focused one): refit and redraw, don't focus.
  useEffect(() => {
    if (!visible || isActive) return;
    const id = requestAnimationFrame(() => {
      try {
        fitAddonRef.current?.fit();
        const t = xtermRef.current;
        if (t) t.refresh(0, Math.max(0, t.rows - 1));
      } catch { /* not laid out yet — the ResizeObserver refits */ }
    });
    return () => cancelAnimationFrame(id);
  }, [visible, isActive]);

  // Parent-driven re-focus. The effect above only fires when the ACTIVE
  // terminal CHANGES (isActive false→true); this handles the selections
  // that don't — re-clicking the current tab, re-selecting this server tab,
  // or clicking a tab while focus was in a tool pane — by reacting to a
  // bumped `focusSignal`. The signal is shared by all of a session's
  // terminals, so gate on isActiveRef: only the active one should grab it.
  const focusSignalRef = useRef(focusSignal);
  useEffect(() => {
    if (focusSignal === focusSignalRef.current) return; // mount / no real change
    focusSignalRef.current = focusSignal;
    const id = requestAnimationFrame(() => {
      if (isActiveRef.current) focusTerminal();
    });
    return () => cancelAnimationFrame(id);
  }, [focusSignal]);

  // Ctrl+F / Cmd+F opens the search chip — but only when the user is
  // focused inside this terminal's DOM subtree. Document-level listener is
  // needed because xterm's textarea steals keyboard focus, so the wrapper
  // never sees the keydown via bubbling within React's synthetic tree.
  // Gate on `document.activeElement` so multiple terminals in tabs / split
  // panes don't all pop their search chip on a single Ctrl+F.
  //
  // The same gate is reused for Ctrl+R / Cmd+R which opens the command
  // history overlay. Browsers normally reload the page on Ctrl+R; inside
  // the Tauri webview a document-level preventDefault stops that dead.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      // Where the terminal is now: its tab, or a Wall's tile.
      const wrapper = dockElRef.current;
      if (!wrapper) return;
      const active = document.activeElement;
      if (!(active instanceof Node) || !wrapper.contains(active)) return;
      if (e.key === 'f' || e.key === 'F') {
        e.preventDefault();
        setShowSearch(true);
        // If already open, refocus the input so a second Ctrl+F selects the
        // existing query for a quick overwrite.
        requestAnimationFrame(() => {
          searchInputRef.current?.focus();
          searchInputRef.current?.select();
        });
      } else if ((e.key === 'r' || e.key === 'R') && !local) {
        // Servers' command history. A local shell keeps Ctrl+R for its own
        // reverse search (bash, PowerShell's PSReadLine).
        e.preventDefault();
        setShowHistory(true);
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, []);

  // Close + clear search decorations. Refocuses xterm so the user's next
  // keystroke goes to the shell, not to nowhere.
  const closeSearch = () => {
    const addon = searchAddonRef.current;
    // Drop the match highlights.
    addon?.clearDecorations();
    setShowSearch(false);
    // Return focus to xterm so keystrokes resume typing at the prompt.
    xtermRef.current?.focus();
  };

  const runFind = (direction: 'next' | 'prev') => {
    const addon = searchAddonRef.current;
    if (!addon || !query) return;
    if (direction === 'next') addon.findNext(query, SEARCH_OPTIONS);
    else addon.findPrevious(query, SEARCH_OPTIONS);
  };

  // Tiny inline toast for clipboard feedback (copy / paste / errors). Pattern
  // matches the per-component notify() used in SftpWorkspace / FilePanel —
  // keeps the component self-contained without a global toast provider.
  const [toast, setToast] = useState<{ msg: string; tone: "ok" | "err" } | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const notify = (msg: string, tone: "ok" | "err" = "ok") => {
    setToast({ msg, tone });
    if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setToast(null), 1400);
  };

  // The color scheme's background, for the padding around the grid.
  const [background, setBackground] = useState(() => schemeForServer(serverId).theme.background);
  // A link in the output was clicked (a plain URL, or an OSC 8 hyperlink,
  // whose text can differ from where it goes): show the real target first,
  // and open only an http(s) link, in the system browser (open_external_url
  // refuses anything else) rather than inside the app. The themed confirm, as
  // window.confirm() does nothing in Android's WebView; Cancel has the focus.
  const openLink = (uri: string) => {
    void (async () => {
      if (!/^https?:\/\//i.test(uri)) return;
      let host = uri;
      try { host = new URL(uri).host; } catch { return; }
      // A very long target is cut so the dialog's buttons stay on screen;
      // the host line still says where it goes.
      const shown = uri.length > 300 ? `${uri.slice(0, 300)}…` : uri;
      const ok = await confirmRef.current({
        title: 'Open this link?',
        message: `A program in this terminal links to:\n\n${shown}\n\nIt opens in your browser, at ${host}.`,
        okLabel: 'Open',
        safeDefault: true,
      });
      if (!ok) return;
      try {
        await invoke('open_external_url', { url: uri });
      } catch (e) {
        notify(`Couldn't open the link: ${String(e)}`, 'err');
      }
    })();
  };

  // The output ends in a password prompt (sudo, su, …): a button offers to
  // type a saved password there (#28). The next key typed or more output
  // takes it away. The backend types the password: it never comes here.
  const [atPasswordPrompt, setAtPasswordPrompt] = useState(false);
  const atPasswordPromptRef = useRef(false);
  const showPasswordButton = (on: boolean) => {
    if (atPasswordPromptRef.current === on) return;
    atPasswordPromptRef.current = on;
    setAtPasswordPrompt(on);
    if (!on) setPasswordMenu(null);
  };
  type SavedPassword = { label: string; detail: string; serverId?: number; credentialId?: number };
  const [passwordMenu, setPasswordMenu] = useState<SavedPassword[] | null>(null);
  // Which saved passwords are offered (the fork's rule, FORK.md): this
  // server's own password and the saved login it uses, when the prompt asks
  // for their user or names nobody (su); another saved login only when the
  // prompt names its user. So the text of a prompt can't get the menu to
  // offer a login's password the prompt has nothing to do with; the user
  // still picks what's typed. The backend also types a server's own password
  // only into that server's own terminals (type_saved_password); a saved
  // login's goes where the user picks it.
  const openPasswordMenu = async () => {
    try {
      const [servers, logins] = await Promise.all([
        invoke<any[]>('get_servers'),
        invoke<any[]>('get_credentials'),
      ]);
      const asked = passwordPromptUser(recentOutputRef.current);
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
      setPasswordMenu(items);
    } catch (e) {
      notify(`Couldn't read the saved passwords: ${String(e)}`, 'err');
    }
  };
  const typeSavedPassword = async (p: SavedPassword) => {
    setPasswordMenu(null);
    try {
      await invoke('type_saved_password', {
        terminalId,
        serverId: p.serverId ?? null,
        credentialId: p.credentialId ?? null,
      });
      showPasswordButton(false);
    } catch (e) {
      notify(String(e), 'err');
    }
    xtermRef.current?.focus();
  };

  useEffect(() => {
    if (!terminalRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: readFontSize(),
      scrollback: readScrollback(),
      // Start on the always-installed fallback stack; the chosen face is
      // switched in below once it has loaded (handleSettingsChange). A bundled
      // font isn't loaded yet at this point, and xterm measures its cell size
      // from whatever face renders first — measuring the fallback and then
      // drawing the real font would leave gaps or overlapping glyphs.
      fontFamily: DEFAULT_FONT_STACK,
      // The server's own scheme, else this device's (Settings › Terminal);
      // handleSettingsChange below follows later changes to either.
      theme: schemeForServer(serverId).theme,
      // Text a program colored too close to its background is lightened or
      // darkened until it reads (WCAG AA, as VS Code's terminal does).
      minimumContrastRatio: readMinContrast() ? MIN_CONTRAST_RATIO : 1,
      // Bold text in the first eight colors uses their bright versions.
      drawBoldTextInBrightColors: readBoldBright(),
      // OSC 8 hyperlinks a program on the server prints: asked first
      // (openLink). xterm already ignores other schemes while
      // allowNonHttpProtocols is off.
      linkHandler: { allowNonHttpProtocols: false, activate: (_event, uri) => openLink(uri) },
      ...(local && IS_WINDOWS ? { windowsPty: { backend: 'conpty' as const } } : {}),
      allowProposedApi: true
    });
    let ptyDisposed = false;
    if (local && IS_WINDOWS) {
      void readWindowsBuild().then((buildNumber) => {
        if (buildNumber && !ptyDisposed) term.options.windowsPty = { backend: 'conpty', buildNumber };
      });
    }

    const fitAddon = new FitAddon();
    fitAddonRef.current = fitAddon;
    term.loadAddon(fitAddon);
    const searchAddon = new SearchAddon();
    searchAddonRef.current = searchAddon;
    term.loadAddon(searchAddon);
    // Unicode 11 character widths, so emoji and CJK take the two cells the
    // shell expects and the cursor stays where it should after them.
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = '11';
    // Plain URLs in the output (not only OSC 8 links) are clickable too.
    term.loadAddon(new WebLinksAddon((_event, uri) => openLink(uri)));
    // A runaway count (COUNTED_FINALS) is handled here by doing nothing;
    // otherwise xterm's own handler runs. The first one is noted in the log.
    let runawayLogged = false;
    // Refilled after each batch is processed (`processed` below).
    let lineBudget = LINE_BUDGET;
    let charBudget = CHAR_BUDGET;
    const countLimit = (final: string) =>
      final === 'b' ? term.rows * term.cols : LINE_FINALS.has(final) ? term.rows * 4 : term.cols * 4;
    // The cell a repeat copies, in code units: the one the last character
    // went into, by its width back from the cursor — 1, 2 (wide), or 0 when
    // it didn't fit at the edge with wrapping off. Which one isn't public,
    // so the longest of the three.
    const repeatedLength = () => {
      const b = term.buffer.active;
      const line = b.getLine(b.baseY + b.cursorY);
      let len = 1;
      for (let x = b.cursorX - 2; x <= b.cursorX; x++) len = Math.max(len, line?.getCell(x)?.getChars().length ?? 0);
      return len;
    };
    for (const final of COUNTED_FINALS) {
      term.parser.registerCsiHandler({ final }, (params) => {
        const count = params[0];
        if (typeof count !== 'number' || count <= 1) return false;
        if (LINE_FINALS.has(final)) {
          if (count <= countLimit(final) && count <= lineBudget) {
            lineBudget -= count;
            return false;
          }
        } else {
          const len = final === 'b' ? repeatedLength() : 1;
          const cost = count * len;
          if (len <= 2 && count <= countLimit(final) && cost <= charBudget) {
            charBudget -= cost;
            return false;
          }
        }
        if (!runawayLogged) {
          runawayLogged = true;
          logLine('warn', `[terminal] ${terminalId}: ignored ESC[${count}${final} and others like it (counts that large, or that many, would freeze the window)`);
        }
        return true;
      });
    }
    // xterm draws into a div of its own, which a Wall can borrow (see
    // src/ui/terminalDock.ts) — so the mouse, resize and focus handling
    // below is bound to it, not to this component's wrapper.
    const home = terminalRef.current;
    const dock = document.createElement('div');
    dock.style.position = 'relative';
    dock.style.width = '100%';
    dock.style.height = '100%';
    const host = document.createElement('div');
    host.style.width = '100%';
    host.style.height = '100%';
    dock.appendChild(host);
    home.appendChild(dock);
    dockElRef.current = dock;
    setDockEl(dock);
    term.open(host);

    // Draw with the GPU (xterm's WebGL renderer) unless Settings turned it
    // off. If WebGL isn't there, or the context is lost — a webview keeps
    // only so many, so the oldest terminals lose theirs when many are open —
    // xterm goes back to drawing with the DOM, which always works.
    let webgl: WebglAddon | null = null;
    // Lost its context once: stays on the DOM renderer until GPU drawing is
    // turned off and on again, rather than taking a context back on the next
    // settings change and so pushing another terminal's out.
    let gpuLost = false;
    const setGpu = (on: boolean) => {
      if (!on) gpuLost = false;
      if (on === !!webgl || (on && gpuLost)) return;
      if (!on) {
        webgl?.dispose();
        webgl = null;
        noteTerminalRenderer(terminalId, "dom");
        return;
      }
      try {
        const addon = new WebglAddon();
        addon.onContextLoss(() => {
          addon.dispose();
          if (webgl === addon) webgl = null;
          gpuLost = true;
          noteTerminalRenderer(terminalId, "dom");
          console.warn(`[terminal] ${terminalId} lost its GPU context; drawing without the GPU`);
        });
        term.loadAddon(addon);
        webgl = addon;
        noteTerminalRenderer(terminalId, "gpu");
      } catch {
        // No WebGL here: don't try again on every settings change.
        webgl = null;
        gpuLost = true;
        noteTerminalRenderer(terminalId, "dom");
      }
    };
    setGpu(readGpu());
    
    // Cleared on unmount: a terminal closed within these 50ms must not open
    // its PTY after the close was sent.
    const openTimer = window.setTimeout(() => {
      fitAddon.fit();
      
      // Start PTY Session with correct dimensions only ONCE.
      if (!openedRef.current) {
        openedRef.current = true;
        // Branch: a `containerExec` prop means we want a docker-exec
        // session into a specific container, not the regular login
        // shell. Same xterm wiring, different backend command — the
        // PTY/data/resize event topology is identical so xterm doesn't
        // notice the difference.
        // The backend only takes resizes once the shell is up, so a refit
        // that landed while it was starting (e.g. the chosen font finished
        // loading and changed the cell size) is sent again once it's open.
        const syncSize = () => {
          invoke(commands.resize, { terminalId, cols: term.cols, rows: term.rows }).catch(() => {});
        };
        const opened = () => { markTyped(terminalId); ptyLiveRef.current = true; terminalStateChanged(); };
        if (telnet) {
          invoke('telnet_open_terminal', {
            sessionId,
            terminalId,
            cols: term.cols || 80,
            rows: term.rows || 24,
            onOutput: outputChannel(),
          }).then(() => { syncSize(); opened(); }, e => {
            term.writeln(`\x1b[31mFailed to open terminal: ${e}\x1b[0m`);
          });
        } else if (local) {
          invoke<string>('local_terminal_open', {
            terminalId,
            shell: local.shell,
            cols: term.cols || 80,
            rows: term.rows || 24,
            onOutput: outputChannel(),
          }).then((label) => {
            syncSize();
            local.onOpened?.(label);
          }, e => {
            term.writeln(`\x1b[31mCouldn't start the shell: ${e}\x1b[0m`);
            // Nothing to type into: keys stop here (the tab marks it ended).
            disabledRef.current = true;
            local.onFailed?.();
          });
        } else if (containerExec) {
          // Through root, asking for the password when sudo or dzdo wants one.
          rootInvoke<void>('open_container_terminal', {
            sessionId,
            terminalId,
            container: containerExec.container,
            cols: term.cols || 80,
            rows: term.rows || 24,
            useSudo: containerExec.useSudo,
            onOutput: outputChannel(),
          }, 'Open the container terminal').then((r) => {
            if (r === null) { term.writeln(ROOT_CANCELLED); return; }
            syncSize(); opened();
          }, e => {
            term.writeln(`\x1b[31mFailed to attach to container: ${e instanceof Error ? e.message : e}\x1b[0m`);
          });
        } else {
          // A terminal with a login of its own logs in first.
          (ownLogin ? connectOwn(term) : Promise.resolve(sessionId)).then((sid) => {
            if (sid === null) return false;
            shellConnRef.current = sid;
            return invoke('open_terminal', {
              sessionId: sid,
              terminalId,
              cols: term.cols || 80,
              rows: term.rows || 24,
              useLocalShell: readUseLocalShell(),
              onOutput: outputChannel(),
            }).then(() => true);
          }).then((ok) => { if (ok) { syncSize(); opened(); watchForShellReady(); } }, e => {
            term.writeln(`\x1b[31mFailed to open terminal: ${e}\x1b[0m`);
          });
        }
      }
    }, 50);

    xtermRef.current = term;

    // Handle Input — swallow keystrokes once the session is disconnected so
    // they don't pile up against a dead backend channel. When a mobile-key-bar
    // modifier is armed/locked we transform single printable characters into
    // the matching control sequence before sending (Ctrl+letter → 0x01-0x1a,
    // Alt+char → ESC-prefix). Multi-char inputs (paste, IME composition) are
    // forwarded verbatim because we can't sensibly "Ctrl" a phrase.
    const handleData = (data: string) => {
      if (disabledRef.current) return;
      // A Wall's line isn't this one's whole any more — unless this is only
      // the terminal saying it got or lost the focus (for vim, tmux).
      if (data !== '\x1b[I' && data !== '\x1b[O') markTyped(terminalId);
      showPasswordButton(false);
      // A paste: our right-click path, or xterm's own keyboard paste, which
      // starts with the bracketed-paste marker when the remote program asked
      // for one. Never run through the armed modifiers or into history.
      const isPaste = pastingRef.current || data.startsWith("\x1b[200~");
      const mods = modifiersRef.current;
      const hasMod = mods.ctrl !== "off" || mods.alt !== "off";
      let bytes: number[];
      if (hasMod && data.length === 1 && !isPaste) {
        const code = data.charCodeAt(0);
        let ch = code;
        if (mods.ctrl !== "off") {
          if ((code >= 0x41 && code <= 0x5a) || (code >= 0x61 && code <= 0x7a)) {
            // Letters → C0 control (Ctrl+A = 0x01 … Ctrl+Z = 0x1a). `code & 0x1f`
            // does the right thing for both upper- and lower-case letters.
            ch = code & 0x1f;
          }
          // Non-letters under Ctrl are left as-is — Android's soft keyboard
          // rarely lets the user type the ones with defined C0 mappings
          // (Ctrl+@, Ctrl+[, Ctrl+\, Ctrl+] etc.) anyway, and silently
          // mangling normal punctuation while a Ctrl chip is lit would
          // confuse more than help.
        }
        bytes = [];
        if (mods.alt !== "off") bytes.push(0x1b); // ESC prefix → Meta
        bytes.push(ch);
        consumeArmedModifiers();
      } else {
        bytes = Array.from(new TextEncoder().encode(data));
      }
      invoke(commands.write, {
        terminalId,
        data: bytes,
      }).catch(console.error);
      // A local shell keeps no history.
      if (local) return;

      // ── Command history capture (best-effort) ────────────────────────────
      // Bufffer up bytes until we see a CR (Enter), then log the accumulated
      // line if it looks like a real command. Purely local heuristic — we do
      // not parse the shell's echo, so arrow-key edits will over-count or
      // mis-count. Skip anything under 3 chars to weed out "y\r" prompts and
      // stray Enters.
      //
      // Pasted text isn't recorded: a multi-line paste into an editor would
      // log every line (and, past the 1,000-entry cap, push out the real
      // history), and a pasted token or key isn't something to keep. A paste
      // that doesn't end in a line break leaves its last line on the prompt,
      // so that line's Enter is skipped too.
      if (isPaste) {
        const body = data.replace(/^\x1b\[200~/, "").replace(/\x1b\[201~$/, "");
        commandBufRef.current = "";
        commandBufPastedRef.current = !/[\r\n]$/.test(body);
        return;
      }
      for (let i = 0; i < data.length; i++) {
        const ch = data.charCodeAt(i);
        if (ch === 0x0d /* CR */ || ch === 0x0a /* LF */) {
          const line = commandBufRef.current.trim();
          const pasted = commandBufPastedRef.current;
          commandBufRef.current = "";
          commandBufPastedRef.current = false;
          // Do NOT persist what was typed at a no-echo password/passphrase
          // prompt — otherwise sudo/su/mysql -p/gpg secrets land verbatim in
          // searchable history. The onData handler can't see the PTY echo
          // mode, so we infer it from the remote output tail.
          if (!pasted && line.length >= 3 && !looksLikePasswordPrompt(recentOutputRef.current)) {
            invoke('cmd_history_add', {
              serverId: serverId > 0 ? serverId : null,
              serverName: serverName || "",
              command: line,
            }).catch(() => { /* silent — history logging is best-effort */ });
          }
        } else if (ch === 0x7f /* DEL */ || ch === 0x08 /* BS */) {
          commandBufRef.current = commandBufRef.current.slice(0, -1);
        } else if (ch === 0x03 /* Ctrl+C */ || ch === 0x15 /* Ctrl+U */) {
          // Reset the buffer — the shell just cleared the line.
          commandBufRef.current = "";
          commandBufPastedRef.current = false;
        } else if (ch === 0x1b /* ESC — arrow keys etc. */) {
          // Skip the whole ESC sequence rather than treating the following
          // bytes as literal characters. Simple heuristic:
          //   ESC [ ... <final>   → CSI, final byte is 0x40-0x7e
          //   ESC O <char>         → SS3 (function keys); consume one byte
          //   ESC <char>           → any other simple sequence; consume one
          const nxt = i + 1 < data.length ? data.charCodeAt(i + 1) : -1;
          if (nxt === 0x5b /* [ */) {
            i += 1; // consume '['
            while (i + 1 < data.length) {
              const fin = data.charCodeAt(i + 1);
              i++;
              if (fin >= 0x40 && fin <= 0x7e) break;
            }
          } else if (nxt !== -1) {
            i += 1; // consume the byte after ESC
          }
        } else if (ch >= 0x20 && ch <= 0x7e) {
          commandBufRef.current += data[i];
          if (commandBufRef.current.length > 4096) {
            // Cap to keep pathological pastes from ballooning memory.
            commandBufRef.current = commandBufRef.current.slice(-4096);
          }
        }
        // Other control chars are ignored on the assumption they don't
        // contribute to the user's visible command line.
      }
    };
    // The mobile key bar's arrow and symbol keys come through here too, so
    // they're recorded exactly like typed input.
    inputRef.current = handleData;
    const onDataDisposable = term.onData(handleData);

    // ---- Copy on select / paste on right-click --------------------------------
    // Selection-change fires per mouse move during a drag — that's noisy AND
    // it would clobber the clipboard mid-drag. We instead wait for the user
    // to release the mouse, then copy whatever is currently selected.
    const writeClipboard = async (text: string) => {
      try {
        await navigator.clipboard.writeText(text);
        return true;
      } catch {
        return false;
      }
    };
    // term.getSelection() emits one '\n' per buffer row, including rows that
    // are mid-wrap continuations of the previous logical line — paste that
    // into anything and you get a phantom blank between every wrapped
    // segment. Walk the buffer ourselves using `isWrapped` so a wrapped
    // line round-trips as a single line. Plain (non-wrapped) row breaks
    // stay '\n'.
    const buildSelectedText = (): string => {
      const range = term.getSelectionPosition();
      if (!range) return '';
      const buf = term.buffer.active;
      const lines: string[] = [];
      for (let y = range.start.y; y <= range.end.y; y++) {
        const line = buf.getLine(y);
        if (!line) continue;
        const startX = y === range.start.y ? range.start.x : 0;
        const endX   = y === range.end.y   ? range.end.x   : undefined;
        const text = line.translateToString(true, startX, endX);
        if (y > range.start.y && line.isWrapped && lines.length > 0) {
          lines[lines.length - 1] += text;
        } else {
          lines.push(text);
        }
      }
      return lines.join('\n');
    };
    const copySelectionIfAny = async () => {
      // A selection that runs past the last line ends in empty rows: Ctrl+Shift+A
      // takes the whole buffer, the blank rows under the prompt included.
      const text = buildSelectedText().replace(/\n+$/, '');
      if (!text) return;
      const ok = await writeClipboard(text);
      if (ok) notify('Copied');
    };
    // Copy-on-select must fire wherever the mouse is RELEASED — not only inside
    // the terminal. A user who drags a selection and lets go past the terminal's
    // edge (a very natural motion) still expects the copy, but the old handler
    // listened for `mouseup` on the terminal element itself, so a release
    // outside it never fired and only left the text selected.
    //
    // Instead: ARM on a left-button mousedown that starts inside this terminal,
    // then catch `mouseup` on the DOCUMENT so the release is seen anywhere.
    // Requiring the drag to have started here keeps an unrelated release
    // elsewhere from touching the clipboard, and copySelectionIfAny() no-ops
    // when there's no real selection — so a plain click (which clears the
    // selection) still can't clobber it. The selection end is set by the last
    // mousemove, so it's final by the time we read it on mouseup.
    let selectionArmed = false;
    const onMouseDown = (ev: MouseEvent) => {
      // Assign (not just set-true) so a non-left mousedown re-disarms a flag
      // that got stuck from an earlier drag whose mouseup we never saw.
      selectionArmed = ev.button === 0;
    };
    const onDocMouseUp = (ev: MouseEvent) => {
      if (ev.button !== 0 || !selectionArmed) return;
      selectionArmed = false;
      void copySelectionIfAny();
    };
    // If the window loses focus mid-drag (Alt-Tab, a focus-stealing dialog),
    // the button is released over another window and the matching `mouseup`
    // never reaches `document`, leaving selectionArmed stuck true. A later
    // unrelated left-click would then copy this terminal's stale selection and
    // clobber the clipboard. Disarm on blur so only a release we actually see
    // can copy.
    const onWindowBlur = () => { selectionArmed = false; };
    // Every paste reaches the terminal here: the clipboard, xterm's own paste,
    // and a Wall's Broadcast. No escape sequence or other control character
    // gets through (see pasteText.ts); the callers clean it first so they can
    // say so and show what will be pasted, and this is the last check.
    const pasteText = (text: string) => {
      const clean = withoutPasteControls(text);
      if (!clean) return;
      pastingRef.current = true;
      try {
        term.paste(clean);
      } finally {
        pastingRef.current = false;
      }
    };
    // Paste the clipboard into the PTY. Disabled sessions silently drop the
    // paste (matches onData).
    const pasteClipboard = async (toast: boolean) => {
      if (disabledRef.current) return;
      let text = '';
      // From the backend on Linux and Windows (util/clipboard), whose error
      // says why; the page's own read only fails when it's refused.
      try { text = await readClipboardText(); }
      catch (e) { notify(typeof e === 'string' ? e : 'Clipboard read denied', 'err'); return; }
      await pasteChecked(text, toast);
    };
    // Every paste of the user's goes through here, however it came (the
    // right-click, the Linux paste keys, the webview's own Ctrl+V,
    // Ctrl+Shift+V or ⌘V, a phone's paste menu): cleaned, and asked about
    // when its lines would run one by one.
    const pasteChecked = async (raw: string, toast: boolean) => {
      if (disabledRef.current) return;
      let text = raw;
      if (!text) return;
      // No escape sequence or other control character gets through (see pasteText.ts).
      const clean = withoutPasteControls(text);
      if (clean !== text) { text = clean; notify('Control characters left out of the paste'); }
      if (!text) return;
      // More than one line into a program that hasn't turned bracketed-paste
      // mode on (an older bash, a plain sh): each line would run the moment it
      // lands, so ask first and show what would run. With the mode on, the
      // program takes the paste as one block and nothing runs by itself; a
      // single line with a trailing newline runs as one command, as anywhere.
      const lines = text.replace(/(\r\n|\r|\n)$/, '').split(/\r\n|\r|\n/);
      if (lines.length > 1 && !term.modes.bracketedPasteMode) {
        const preview = lines.slice(0, 4).map((l) => (l.length > 70 ? `${l.slice(0, 70)}…` : l)).join('\n');
        const ok = await confirmRef.current({
          title: `Paste ${lines.length} lines?`,
          message: `The program here doesn't take a paste as one block, so each line runs as soon as it lands:\n\n${preview}${lines.length > 4 ? `\n… and ${lines.length - 4} more` : ''}`,
          okLabel: 'Paste',
          safeDefault: true,
        });
        if (!ok || disabledRef.current) return;
      }
      // Hand the text to xterm's own paste path instead of writing it raw to
      // the PTY. xterm collapses CRLF / lone LF to a single CR (one Enter per
      // line break — a Windows clipboard's `\r\n` would otherwise be two) and,
      // when the remote program has turned on bracketed-paste mode (vim,
      // bash ≥ 5.1, zsh, …), wraps the text in ESC[200~ … ESC[201~ so the
      // program knows it was pasted. The raw write skipped those brackets, so
      // vim treated every pasted line as typed input and auto-indented each
      // one on top of the previous (#56), and a shell ran each pasted line the
      // moment it arrived. Going through xterm also routes the paste through
      // onData like every other input; `pastingRef` keeps it out of history
      // and the armed modifiers.
      pasteText(text);
      if (toast) notify('Pasted');
    };
    // Ctrl+V, Ctrl+Shift+V, Shift+Insert, ⌘V and a phone's paste menu would
    // reach xterm's own paste, which pastes at once. Taken here instead (the
    // capture phase runs before xterm's handler) and pasted the way a
    // right-click pastes: without control characters, and asking first when
    // its lines would run one by one.
    const onPasteEvent = (ev: ClipboardEvent) => {
      const raw = ev.clipboardData?.getData('text/plain') ?? '';
      ev.preventDefault();
      ev.stopImmediatePropagation();
      void pasteChecked(raw, false);
    };
    // Right-click → paste. preventDefault swallows the platform context menu
    // so the user gets the terminal-style behavior they asked for.
    const onContextMenu = (ev: MouseEvent) => {
      ev.preventDefault();
      void pasteClipboard(true);
    };
    // One key handler (xterm allows only one):
    //  - the app's own shortcuts (broadcast on/off, pause) pass by the shell —
    //    DesktopApp acts on them;
    //  - Ctrl+Shift+A / Ctrl+Shift+C select all / copy, on the SHIFTED combo
    //    so plain Ctrl+A (start of line) and Ctrl+C (SIGINT) still reach the
    //    shell, as every readline/zle user expects;
    //  - Linux: the paste shortcuts (Ctrl+Shift+V, Shift+Insert) are taken
    //    from the webview, whose own paste crashes on an empty clipboard, and
    //    go through the same path as a right-click.
    term.attachCustomKeyEventHandler((ev) => {
      if (appHotkey(ev)) return false;
      const noOtherMods = !ev.altKey && !ev.metaKey && ev.ctrlKey && ev.shiftKey;
      if (noOtherMods && ev.code === 'KeyA') {
        if (ev.type === 'keydown' && !ev.repeat) term.selectAll();
        ev.preventDefault();
        return false;
      }
      if (noOtherMods && ev.code === 'KeyC') {
        if (ev.type === 'keydown' && !ev.repeat) void copySelectionIfAny();
        ev.preventDefault();
        return false;
      }
      if (IS_LINUX) {
        const isPasteKey = !ev.altKey && !ev.metaKey && (
          (ev.ctrlKey && ev.shiftKey && ev.code === 'KeyV') ||
          (ev.shiftKey && !ev.ctrlKey && ev.key === 'Insert')
        );
        if (isPasteKey) {
          ev.preventDefault();
          if (ev.type === 'keydown' && !ev.repeat) void pasteClipboard(false);
          return false;
        }
      }
      return true;
    });
    host.addEventListener('mousedown', onMouseDown);
    document.addEventListener('mouseup', onDocMouseUp);
    window.addEventListener('blur', onWindowBlur);
    host.addEventListener('contextmenu', onContextMenu);
    host.addEventListener('paste', onPasteEvent, true);

    // Handle Resize, once the size has settled (PTY_SIZE_SETTLE_MS).
    // Refused until the shell is open (NOT_OPEN); the size is sent again
    // once it is (syncSize), so that's not worth an error in the log.
    let sizeTimer = 0;
    const onResizeDisposable = term.onResize(() => {
      window.clearTimeout(sizeTimer);
      sizeTimer = window.setTimeout(() => {
        invoke(commands.resize, { terminalId, cols: term.cols, rows: term.rows }).catch(() => {});
      }, PTY_SIZE_SETTLE_MS);
    });

    // Output: raw bytes over the channel the terminal was opened with. Once
    // xterm has processed a batch it's acknowledged, which is what lets the
    // backend send more (src-tauri/src/term_output.rs): a stream faster than
    // this window can draw waits there instead of piling up here. Acks are
    // grouped, at most one per ACK_EVERY bytes or 30ms, and each names the
    // channel its output came over.
    const ACK_EVERY = 128 * 1024;
    let unacked = 0;
    let ackChannel = 0;
    let ackTimer = 0;
    const sendAck = () => {
      if (ackTimer) window.clearTimeout(ackTimer);
      ackTimer = 0;
      const bytes = unacked;
      unacked = 0;
      if (bytes > 0) invoke('terminal_output_ack', { terminalId, channel: ackChannel, bytes }).catch(() => {});
    };
    const processed = (channel: number, bytes: number) => {
      lineBudget = LINE_BUDGET;
      charBudget = CHAR_BUDGET;
      noteTerminalParsed(terminalId, bytes);
      if (channel !== ackChannel) { sendAck(); ackChannel = channel; }
      unacked += bytes;
      if (unacked >= ACK_EVERY) sendAck();
      else if (!ackTimer) ackTimer = window.setTimeout(sendAck, 30);
    };
    // Highlighting: the server's own set, or this device's (applyLook
    // follows changes to either). Off, output goes to xterm as it came.
    const highlighter = new HighlightStream((late) => term.write(late));
    highlightRef.current = highlighter;
    let matcher: Matcher | null = matcherForServer(serverId);
    highlighter.setMatcher(matcher, false);
    outputRef.current = (data: Uint8Array, channel: number) => {
      const w = readyWatchRef.current;
      if (w) { window.clearTimeout(w.quiet); w.quiet = window.setTimeout(shellReady, SHELL_QUIET_MS); }
      // Counted for the freeze watchdog: received, and still to be drawn.
      // xterm buffers a multibyte sequence split across batches.
      noteTerminalOutput(terminalId, data.length);
      if (!local) noteOutput(terminalId, data);
      if (highlighter.active) term.write(highlighter.push(data), () => processed(channel, data.length));
      else term.write(data, () => processed(channel, data.length));
      // Keep a short tail of decoded output so command-history capture can
      // recognize a no-echo password prompt and skip the typed secret.
      try {
        const txt = new TextDecoder().decode(data);
        const combined = recentOutputRef.current + txt;
        recentOutputRef.current = combined.length > 512 ? combined.slice(-512) : combined;
        showPasswordButton(looksLikePasswordPrompt(recentOutputRef.current));
      } catch { /* decode hiccup on a split multibyte chunk — ignore */ }
    };

    // Coalesce resize bursts via rAF, and skip the refit on hidden tabs
    // so SIGWINCH doesn't leak to background PTYs (the become-visible
    // effect refits when the tab is shown again).
    let rafId = 0;
    const onScreen = () => isActiveRef.current || visibleRef.current || dockedRef.current;
    const resizeObserver = new ResizeObserver(() => {
      if (!xtermRef.current) return;
      if (!onScreen()) return;
      // A terminal at its bottom stays there. On a phone, the box changing
      // while output comes in (the key bar back after a reconnect, the
      // keyboard) is at times taken by xterm for the user scrolling up, and
      // the view then stops following what comes in: a reconnected session
      // kept showing the old one's last lines, its new prompt out of sight
      // below. Put it back at the bottom once the change has settled.
      const buf = xtermRef.current.buffer.active;
      if (buf.viewportY === buf.baseY) {
        requestAnimationFrame(() => requestAnimationFrame(() => xtermRef.current?.scrollToBottom()));
      }
      if (rafId) return; // a rAF is already queued; coalesce
      rafId = requestAnimationFrame(() => {
        rafId = 0;
        if (!xtermRef.current) return;
        if (!onScreen()) return;
        try {
          fitAddon.fit();
          const t = xtermRef.current;
          t.refresh(0, Math.max(0, t.rows - 1));
        } catch { /* swallow — xterm tolerates transient 0×0 sizes */ }
      });
    });
    resizeObserver.observe(host);
    
    // Colors, contrast and the renderer apply live to open terminals: a change
    // in Settings, or to the server's own scheme (an edit, or a sync).
    const applyLook = () => {
      const { theme } = schemeForServer(serverId);
      if (term.options.theme !== theme) {
        term.options.theme = theme;
        setBackground(theme.background);
        // A Wall tile showing this terminal repaints its padding to match.
        terminalStateChanged();
      }
      const ratio = readMinContrast() ? MIN_CONTRAST_RATIO : 1;
      if (term.options.minimumContrastRatio !== ratio) term.options.minimumContrastRatio = ratio;
      const boldBright = readBoldBright();
      if (term.options.drawBoldTextInBrightColors !== boldBright) term.options.drawBoldTextInBrightColors = boldBright;
      setGpu(readGpu());
      const next = matcherForServer(serverId);
      if (next !== matcher) {
        matcher = next;
        const held = highlighter.setMatcher(next, term.buffer.active.type === 'alternate');
        if (held) term.write(held);
      }
    };

    // Handle Settings Change — font size and face apply live to open terminals.
    let fontDisposed = false;
    const handleSettingsChange = () => {
      applyLook();
      // So does the scrollback; fewer lines than the terminal holds drops the
      // oldest ones.
      const scrollback = readScrollback();
      if (term.options.scrollback !== scrollback) term.options.scrollback = scrollback;
      const size = readFontSize();
      const family = fontFamilyCss(readFontFamily());
      if (term.options.fontSize === size && term.options.fontFamily === family) return;
      const apply = () => {
        if (fontDisposed) return;
        // A newer change may have loaded first — never let this older one
        // overwrite it.
        if (readFontSize() !== size || fontFamilyCss(readFontFamily()) !== family) return;
        term.options.fontSize = size;
        term.options.fontFamily = family;
        try {
          fitAddon.fit();
          term.refresh(0, Math.max(0, term.rows - 1));
        } catch { /* hidden tab (0×0) — the ResizeObserver refits on show */ }
      };
      // Wait until the face is ready so xterm measures the real glyph width,
      // not the fallback's (a mis-measured cell leaves gaps or overlaps).
      const fonts = (document as any).fonts;
      if (fonts?.load) fonts.load(`${size}px ${family}`).then(apply, apply);
      else apply();
    };
    window.addEventListener('submarine-settings-changed', handleSettingsChange);
    // Apply the saved face now (no-op when it's the default stack).
    handleSettingsChange();

    // ── Mobile QoL ───────────────────────────────────────────────────────────
    // On soft-keyboard open (`visualViewport.resize` shrinks) refit so xterm's
    // row count matches the now-shorter visible area and scroll to the
    // prompt so the cursor row sits just above the kb.
    //
    // We deliberately do NOT hook mousedown / touchstart here. Those fire at
    // the START of a selection drag, and a scrollToBottom() there yanks the
    // viewport out from under the user's finger and destroys the selection
    // anchor (v0.2.30-v0.2.31 regression). xterm already scrolls on user
    // keystrokes via `scrollOnUserInput` (default true) and on PTY writes,
    // so tapping to focus + typing already reveals the prompt without our
    // help. This handler is what actually implements "keyboard opens →
    // prompt in view" and is the only trigger we want.
    //
    // No-op on desktop: visualViewport.resize doesn't fire from window resizes,
    // only virtual-keyboard / pinch-zoom.
    const scrollPromptIntoView = () => {
      requestAnimationFrame(() => {
        try { fitAddon.fit(); } catch { /* terminal not ready */ }
        term.scrollToBottom();
      });
    };

    let lastVvHeight = window.visualViewport?.height ?? window.innerHeight;
    const onVvResize = () => {
      const h = window.visualViewport?.height ?? window.innerHeight;
      if (h < lastVvHeight - 80) {
        // Drop ≥80px almost always means the soft keyboard opened (not a
        // small browser-UI reflow). 80px is comfortably above the URL-bar
        // collapse delta on Chrome/Android.
        scrollPromptIntoView();
      }
      lastVvHeight = h;
    };
    window.visualViewport?.addEventListener('resize', onVvResize);

    // A server terminal can be shown in a Wall and typed to from there.
    // Whether its xterm has the keyboard, by the focus events xterm itself
    // goes by (see `moved`).
    let focused = false;
    const onTextFocus = () => { focused = true; };
    const onTextBlur = () => { focused = false; };
    term.textarea?.addEventListener('focus', onTextFocus);
    term.textarea?.addEventListener('blur', onTextBlur);
    // Server terminals, for Walls; and an AI tab's, which the AI side pane
    // shows beside a server tab. Other Home shells stay in Home.
    const dockable = !local || !!local.shell?.startsWith('ai:');
    const unregister = !dockable ? null : registerTerminal({
      terminalId,
      sessionId,
      term,
      host: dock,
      home,
      live: () => !disabledRef.current && ptyLiveRef.current,
      paste: (text) => {
        if (disabledRef.current || !ptyLiveRef.current) return false;
        pasteText(text);
        return true;
      },
      moved: (docked) => {
        dockedRef.current = docked;
        setLooking(terminalId, lookingNow());
        // Moving the element can take the keyboard from it without a blur
        // event, so xterm would go on drawing the cursor of a terminal being
        // typed in. Only then: xterm tells a program that asked (vim, tmux)
        // about each blur, and one that never had focus mustn't hear it.
        if (focused && term.textarea && document.activeElement !== term.textarea) {
          term.textarea.dispatchEvent(new FocusEvent('blur'));
        }
        requestAnimationFrame(() => {
          try {
            fitAddon.fit();
            term.refresh(0, Math.max(0, term.rows - 1));
          } catch { /* not laid out yet — the ResizeObserver refits */ }
        });
      },
    });

    const unwatch = local ? null : watchTerminal(terminalId, sessionId);
    setLooking(terminalId, lookingNow());

    return () => {
      unwatch?.();
      unregister?.();
      term.textarea?.removeEventListener('focus', onTextFocus);
      term.textarea?.removeEventListener('blur', onTextBlur);
      ptyLiveRef.current = false;
      window.clearTimeout(openTimer);
      stopReadyWatch();
      fontDisposed = true;
      ptyDisposed = true;
      window.removeEventListener('submarine-settings-changed', handleSettingsChange);
      resizeObserver.disconnect();
      onDataDisposable.dispose();
      if (inputRef.current === handleData) inputRef.current = null;
      onResizeDisposable.dispose();
      window.clearTimeout(sizeTimer);
      host.removeEventListener('mousedown', onMouseDown);
      host.removeEventListener('contextmenu', onContextMenu);
      host.removeEventListener('paste', onPasteEvent, true);
      document.removeEventListener('mouseup', onDocMouseUp);
      window.removeEventListener('blur', onWindowBlur);
      window.visualViewport?.removeEventListener('resize', onVvResize);
      if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
      outputRef.current = null;
      highlighter.dispose();
      if (highlightRef.current === highlighter) highlightRef.current = null;
      if (ackTimer) window.clearTimeout(ackTimer);
      searchAddon.dispose();
      searchAddonRef.current = null;
      webgl?.dispose();
      forgetTerminal(terminalId);
      term.dispose();
      dock.remove();
      if (dockElRef.current === dock) dockElRef.current = null;
      setDockEl((cur) => (cur === dock ? null : cur));
      invoke(commands.close, { terminalId }).catch(console.error);
      closeOwn();
    };
  }, [sessionId, terminalId]);

  return (
    // flex-col so the MobileKeyBar can pin to the bottom of the terminal
    // pane (between xterm and the system soft keyboard). On desktop the bar
    // isn't rendered at all, so the xterm child claims the full height as
    // before.
    <div className="h-full w-full p-2 pr-2 pb-0 relative flex flex-col" style={{ background }}>
      <div
        ref={terminalRef}
        className="flex-1 min-h-0 w-full overflow-hidden select-text"
      />
      {/* Over the terminal, inside what a Wall moves (dockEl), so they go
          with it into a tile. */}
      {dockEl && createPortal(
        <>
          {/* Search chip — floats above xterm, so it never displaces the
              terminal grid. Absolute top-2 right-2 keeps it clear of the
              disconnected badge (which centers) and the clipboard toast
              (bottom-right). */}
          {showSearch && (
            <div className="absolute top-2 right-2 z-20 flex items-center gap-1 rounded-md bg-zinc-800 border border-white/10 px-1.5 py-1 shadow-lg">
              <input
                ref={searchInputRef}
                autoFocus
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  // Stop keys from leaking to the document listener (Ctrl+F
                  // reopen loop) and to xterm's textarea (which would type
                  // into the shell).
                  if (e.key === 'Escape') {
                    e.preventDefault();
                    setQuery('');
                    closeSearch();
                  } else if (e.key === 'Enter') {
                    e.preventDefault();
                    runFind(e.shiftKey ? 'prev' : 'next');
                  } else if ((e.ctrlKey || e.metaKey) && (e.key === 'f' || e.key === 'F')) {
                    // Swallow so the outer document handler doesn't re-fire
                    // (which would just refocus us — harmless but noisy).
                    e.preventDefault();
                    (e.currentTarget as HTMLInputElement).select();
                  }
                  e.stopPropagation();
                }}
                placeholder="Find"
                className="bg-transparent outline-none text-[12px] text-zinc-100 placeholder:text-zinc-500 w-40 px-1"
              />
              <button
                type="button"
                onClick={() => runFind('prev')}
                title="Previous match (Shift+Enter)"
                className="px-1.5 py-0.5 text-[11px] text-zinc-300 hover:text-white hover:bg-white/5 rounded"
              >
                {'↑'}
              </button>
              <button
                type="button"
                onClick={() => runFind('next')}
                title="Next match (Enter)"
                className="px-1.5 py-0.5 text-[11px] text-zinc-300 hover:text-white hover:bg-white/5 rounded"
              >
                {'↓'}
              </button>
              <button
                type="button"
                onClick={() => { setQuery(''); closeSearch(); }}
                title="Close (Esc)"
                className="px-1.5 py-0.5 text-[11px] text-zinc-400 hover:text-white hover:bg-white/5 rounded"
              >
                {'✕'}
              </button>
            </div>
          )}
          {atPasswordPrompt && !local && !disabled && !showSearch && (
            <div className="absolute top-2 right-2 z-20 flex flex-col items-end gap-1 max-w-[calc(100%-1rem)]">
              <button
                type="button"
                onClick={() => (passwordMenu ? setPasswordMenu(null) : void openPasswordMenu())}
                aria-haspopup="menu"
                aria-expanded={!!passwordMenu}
                title="Type a saved password at this prompt (no Enter)"
                className="flex items-center gap-1.5 rounded-md bg-zinc-800 border border-white/10 px-2 py-1 shadow-lg text-[11px] text-zinc-200 hover:text-white hover:border-primary/40"
              >
                <KeyRound size={12} className="text-primary shrink-0" />
                <span className="whitespace-nowrap">Saved password</span>
              </button>
              {passwordMenu && (
                <div
                  role="menu"
                  aria-label="Saved passwords"
                  onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); setPasswordMenu(null); xtermRef.current?.focus(); } }}
                  className="w-64 max-w-full max-h-60 overflow-y-auto rounded-md bg-zinc-900 border border-white/10 shadow-xl p-1"
                >
                  {passwordMenu.length === 0 ? (
                    <div className="px-2 py-1.5 text-[11px] text-zinc-400">
                      No saved password for this prompt. This server's own password, or a saved login for the user it asks about, can be typed here.
                    </div>
                  ) : passwordMenu.map((p) => (
                    <button
                      key={p.serverId != null ? `s${p.serverId}` : `c${p.credentialId}`}
                      type="button"
                      role="menuitem"
                      onClick={() => void typeSavedPassword(p)}
                      className="w-full text-left px-2 py-1.5 rounded hover:bg-white/[0.06] focus:bg-white/[0.06] focus:outline-none"
                    >
                      <div className="text-[11.5px] text-zinc-100 truncate">{p.label}</div>
                      {p.detail && <div className="text-[10px] text-zinc-500 truncate">{p.detail}</div>}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
          {/* Clipboard toast — bottom-right of the terminal pane, never wider
              than the pane (a split or a wide tool panel can leave it narrow).
              Pointer-events off so a stray hover never blocks selection /
              right-click. */}
          {toast && (
            <div className="absolute bottom-3 right-3 z-20 max-w-[calc(100%-1.5rem)] pointer-events-none">
              <span
                className={`block truncate px-2.5 py-1 rounded text-[10.5px] font-mono uppercase tracking-wider border ${
                  toast.tone === 'err'
                    ? 'bg-rose-500/15 border-rose-500/30 text-rose-300'
                    : 'bg-emerald-500/15 border-emerald-500/30 text-emerald-300'
                }`}
              >
                {toast.msg}
              </span>
            </div>
          )}
        </>,
        dockEl,
      )}
      {isMobile && !disabled && (
        <MobileKeyBar
          modifiers={modifiers}
          onToggleModifier={toggleModifier}
          onSpecialKey={sendSpecialKey}
        />
      )}
      {!local && <HistorySearchOverlay
        isOpen={showHistory}
        onClose={() => {
          setShowHistory(false);
          // Return focus to xterm so the user can keep typing at the prompt
          // instead of losing keystrokes into the void.
          xtermRef.current?.focus();
        }}
        filterServerId={serverId > 0 ? serverId : null}
        onInsert={(command, execute) => {
          if (disabledRef.current) return;
          const payload = execute ? command + "\r" : command;
          const bytes = Array.from(new TextEncoder().encode(payload));
          markTyped(terminalId);
          invoke(commands.write, { terminalId, data: bytes }).catch(console.error);
          // Mirror the inserted command into the local buffer so the next
          // Enter (if the user typed one manually) still logs correctly.
          if (!execute) {
            commandBufRef.current = command;
            commandBufPastedRef.current = false;
          }
        }}
      />}
      {/* Disabled badge: NON-blocking. xterm stays interactive for scroll +
          mouse selection so the user can copy whatever was on screen at the
          time the session dropped. Keystrokes are still swallowed at the
          source via `disabledRef` in the onData callback — no input reaches
          a dead PTY. `pointer-events-none` keeps the badge from eating the
          select-drag that starts under it. */}
      {/* One line: centred like this it only gets half the width, and wrapped
          it covered two lines of the terminal on a phone, where the banner
          above already says the session is down. */}
      {disabled && (
        <div className="absolute top-2 left-1/2 -translate-x-1/2 z-10 pointer-events-none">
          <span className="whitespace-nowrap px-2.5 py-1 rounded text-[10.5px] font-mono uppercase tracking-wider border bg-rose-500/15 border-rose-500/30 text-rose-300 shadow-lg">
            <span className="sm:hidden">Read-only</span>
            <span className="hidden sm:inline">Disconnected — buffer is read-only</span>
          </span>
        </div>
      )}
    </div>
  );
};

export default TerminalView;
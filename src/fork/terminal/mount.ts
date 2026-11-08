import type { MutableRefObject, RefObject } from 'react';
import type { IBufferRange, ITerminalOptions, Terminal } from '@xterm/xterm';
import { FitAddon as XtermFitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { WebglAddon } from '@xterm/addon-webgl';
import { invoke } from '@tauri-apps/api/core';
import { claimGpu, gpuChanged, releaseGpu, type GpuHolder } from '../../ui/gpuSlots';
import { HighlightStream, matcherForServer, type Matcher } from '../../ui/highlight';
import { noteOutput, setLooking, watchTerminal } from '../../ui/terminalActivity';
import { registerTerminal, settleAfterMove, terminalStateChanged } from '../../ui/terminalDock';
import { MIN_CONTRAST_RATIO, readBoldBright, readGpu, readMinContrast, schemeForServer } from '../../ui/terminalThemes';
import { forgetTerminal, logLine, noteTerminalOutput, noteTerminalParsed, noteTerminalRenderer } from '../../util/diagnostics';
import { withoutPasteControls } from '../../util/pasteText';
import { IS_WINDOWS } from '../../util/platform';
import type { TerminalFork, TerminalProps } from './hooks';
import { CHAR_BUDGET, COUNTED_FINALS, LINE_BUDGET, LINE_FINALS, ROOT_CANCELLED, SHELL_QUIET_MS } from './rules';
import { shareWidths } from './widthCache';

// ─────────────────────────────────────────────────────────────────────────────
// The fork's part of a terminal's xterm, from TerminalView's mount effect
// (#163): one call sets it up, and its methods are the one-line hooks there,
// each where TerminalView did that work, in the same order.
// ─────────────────────────────────────────────────────────────────────────────

/** A FitAddon that leaves a terminal alone while its box isn't laid out: a
 *  terminal behind another tab is left out of the layout (#127), and xterm
 *  would measure it as a few cells and tell the server's shell so. It keeps
 *  its size (80×24 if it opened hidden) and fits once it's shown. */
export class FitAddon extends XtermFitAddon {
  private laidOutTerm: Terminal | null = null;
  activate(terminal: Terminal): void {
    this.laidOutTerm = terminal;
    super.activate(terminal);
  }
  fit(): void {
    const el = this.laidOutTerm?.element;
    if (!el || el.getClientRects().length === 0) return;
    super.fit();
  }
}

/** The look a terminal opens with: the server's own scheme, else this
 *  device's (Settings › Terminal); applyLook follows later changes to either.
 *  Text a program colored too close to its background is lightened or
 *  darkened until it reads (WCAG AA, as VS Code's terminal does); bold text
 *  in the first eight colors uses their bright versions. A Home terminal on
 *  Windows runs in a pseudo-console (ConPTY), which wraps and redraws lines
 *  itself on a resize; xterm matches it when told so. */
export const xtermLook = (serverId: number, local: TerminalProps['local']): ITerminalOptions => ({
  theme: schemeForServer(serverId).theme,
  minimumContrastRatio: readMinContrast() ? MIN_CONTRAST_RATIO : 1,
  drawBoldTextInBrightColors: readBoldBright(),
  ...(local && IS_WINDOWS ? { windowsPty: { backend: 'conpty' as const } } : {}),
});

// ConPTY is matched better for knowing which Windows build it is. Asked for once.
let windowsBuild: Promise<number | null> | null = null;
const readWindowsBuild = () =>
  (windowsBuild ??= invoke<number | null>('local_windows_build').catch(() => null));

// A terminal's output reaches the page over the channel its own view opened
// it with (src-tauri/src/term_output.rs), not as events a Wall tile could
// listen to: the session tab's view hands each batch on to the tiles showing
// the same terminal.
const wallTileOutput = new Map<string, Set<(data: Uint8Array) => void>>();

// A plain URL has no range of its own to hand the terminal's link handler,
// which doesn't read it.
const NO_RANGE: IBufferRange = { start: { x: 0, y: 0 }, end: { x: 0, y: 0 } };

/** Text from outside the terminal (the Library's commands) pasted the way
 *  every paste is: no escape sequence or other control character gets
 *  through (see pasteText.ts), and `pastingRef` keeps it out of history. */
export const pasteClean = (term: Terminal, text: string, pastingRef: MutableRefObject<boolean>) => {
  const clean = withoutPasteControls(text);
  if (!clean) return;
  pastingRef.current = true;
  try {
    term.paste(clean);
  } finally {
    pastingRef.current = false;
  }
};

/** A container's terminal opened through root (rootInvoke): null when the
 *  password wasn't given, and the terminal says so. */
export const rootOpened = (term: Terminal, syncSize: () => void) => (r: void | null) => {
  if (r === null) { term.writeln(ROOT_CANCELLED); return; }
  syncSize();
};

/** Sets up the fork's part of `term`, once it's made and has its fit and
 *  search addons, before it opens: more addons, the guard against runaway
 *  counts, and the div xterm draws into, which the AI side pane can borrow.
 *  From then on `terminalRef` is that div, so the original project's code
 *  opens xterm in it and binds its mouse, resize and focus handling to it. */
export const mountTerminal = (
  term: Terminal,
  fork: TerminalFork,
  { terminalRef, fitAddon, setTermBg }: {
    terminalRef: RefObject<HTMLDivElement>;
    fitAddon: FitAddon;
    setTermBg: (background: string | undefined) => void;
  },
) => {
  const { terminalId, sessionId, local, attachOnly, serverId } = fork;

  let ptyDisposed = false;
  if (local && IS_WINDOWS) {
    void readWindowsBuild().then((buildNumber) => {
      if (buildNumber && !ptyDisposed) term.options.windowsPty = { backend: 'conpty', buildNumber };
    });
  }

  // Unicode 11 character widths, so emoji and CJK take the two cells the
  // shell expects and the cursor stays where it should after them.
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = '11';
  // Plain URLs in the output (not only OSC 8 links) are clickable too, and
  // open the way an OSC 8 link does (the terminal's linkHandler).
  term.loadAddon(new WebLinksAddon((event, uri) => term.options.linkHandler?.activate(event, uri, NO_RANGE)));
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

  // xterm draws into a div of its own, which the AI side pane can borrow
  // (see src/ui/terminalDock.ts) — so the mouse, resize and focus handling
  // is bound to it, not to TerminalView's wrapper.
  const home = terminalRef.current as HTMLDivElement;
  const dock = document.createElement('div');
  dock.style.position = 'relative';
  dock.style.width = '100%';
  dock.style.height = '100%';
  const host = document.createElement('div');
  host.style.width = '100%';
  host.style.height = '100%';
  dock.appendChild(host);
  home.appendChild(dock);
  fork.dockElRef.current = dock;
  fork.setDockEl(dock);
  const ref = terminalRef as MutableRefObject<HTMLDivElement | null>;
  ref.current = host;

  // Draw with the GPU (xterm's WebGL renderer) unless Settings turned it
  // off, while there's a context for it (src/ui/gpuSlots.ts: terminals on
  // screen first). If WebGL isn't there, or the context is lost, xterm goes
  // back to drawing with the DOM, which always works.
  let webgl: WebglAddon | null = null;
  // Lost its context once: stays on the DOM renderer until GPU drawing is
  // turned off and on again, rather than taking a context back on the next
  // settings change and so pushing another terminal's out.
  let gpuLost = false;
  const dropGpu = () => {
    webgl?.dispose();
    webgl = null;
    noteTerminalRenderer(terminalId, "dom");
  };
  const gpuSlot: GpuHolder = {
    onScreen: () => fork.onScreen(),
    evict: dropGpu,
    retry: () => updateGpu(),
  };
  const updateGpu = () => {
    const on = readGpu();
    if (!on) gpuLost = false;
    if (!on || gpuLost) {
      if (webgl) dropGpu();
      releaseGpu(gpuSlot);
      return;
    }
    if (!gpuSlot.onScreen()) {
      // Kept until a terminal on screen needs it; one may be waiting now.
      if (webgl) gpuChanged();
      return;
    }
    if (webgl || !claimGpu(gpuSlot)) return;
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        addon.dispose();
        if (webgl === addon) webgl = null;
        gpuLost = true;
        releaseGpu(gpuSlot);
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
      releaseGpu(gpuSlot);
      noteTerminalRenderer(terminalId, "dom");
    }
  };

  // Output, once TerminalView has its drawn-bytes report (`drew`).
  let highlighter: HighlightStream | null = null;
  let matcher: Matcher | null = null;
  let processed: (bytes: number) => void = () => {};
  let fromOwner: ((data: Uint8Array) => void) | null = null;

  // A terminal can be moved into the AI side pane. Whether its xterm has
  // the keyboard, by the focus events xterm itself goes by (see `moved`).
  let focused = false;
  const onTextFocus = () => { focused = true; };
  const onTextBlur = () => { focused = false; };
  let unregister: (() => void) | null = null;
  let unwatch: (() => void) | null = null;

  return {
    /** Once xterm is open: the GPU, as Settings and the screen allow; and
     *  the DOM renderer's character widths shared with every terminal's. */
    startGpu: () => {
      shareWidths(term);
      fork.gpuUpdateRef.current = updateGpu;
      updateGpu();
    },
    /** What comes over the terminal's channel goes to `handle` (TerminalView's
     *  output handler), after the fork's own bookkeeping: the wait for a new
     *  shell's prompt, the freeze watchdog's count, the activity mark, and
     *  the Wall tiles showing this terminal, which get each batch from here. */
    output: (drew: (n: number) => void, handle: (data: Uint8Array) => void) => {
      processed = (bytes: number) => {
        lineBudget = LINE_BUDGET;
        charBudget = CHAR_BUDGET;
        if (attachOnly) return;
        noteTerminalParsed(terminalId, bytes);
        drew(bytes);
      };
      // Highlighting: the server's own set, or this device's (applyLook
      // follows changes to either). Off, output goes to xterm as it came.
      const stream = new HighlightStream((late) => term.write(late));
      highlighter = stream;
      fork.highlightRef.current = stream;
      matcher = matcherForServer(serverId);
      stream.setMatcher(matcher, false);
      fork.outputRef.current = (data: Uint8Array) => {
        const w = fork.readyWatchRef.current;
        if (w) { window.clearTimeout(w.quiet); w.quiet = window.setTimeout(fork.shellReady, SHELL_QUIET_MS); }
        // Counted for the freeze watchdog: received, and still to be drawn.
        // xterm buffers a multibyte sequence split across batches. The same
        // batch goes to the Wall tiles showing this terminal.
        if (!attachOnly) {
          noteTerminalOutput(terminalId, data.length);
          if (!local) noteOutput(terminalId, data);
          wallTileOutput.get(terminalId)?.forEach((tile) => tile(data));
        }
        handle(data);
      };
      const own = (data: Uint8Array) => fork.outputRef.current?.(data);
      fromOwner = own;
      if (attachOnly) {
        const tiles = wallTileOutput.get(terminalId) ?? new Set();
        tiles.add(own);
        wallTileOutput.set(terminalId, tiles);
      }
    },
    /** One piece of output into xterm, highlighted when that's on. It counts
     *  as drawn once xterm has taken it in; also when xterm turned it away
     *  (past its own limit), so the count stays true. */
    write: (piece: Uint8Array) => {
      try {
        if (highlighter?.active) term.write(highlighter.push(piece), () => processed(piece.length));
        else term.write(piece, () => processed(piece.length));
      } catch { processed(piece.length); }
    },
    /** Colors, contrast, the renderer and highlighting apply live to open
     *  terminals: a change in Settings, or to the server's own scheme or
     *  highlighting (an edit, or a sync). */
    applyLook: () => {
      const { theme } = schemeForServer(serverId);
      if (term.options.theme !== theme) {
        term.options.theme = theme;
        setTermBg(theme.background);
        // The AI side pane showing this terminal repaints its padding to match.
        terminalStateChanged();
      }
      const ratio = readMinContrast() ? MIN_CONTRAST_RATIO : 1;
      if (term.options.minimumContrastRatio !== ratio) term.options.minimumContrastRatio = ratio;
      const boldBright = readBoldBright();
      if (term.options.drawBoldTextInBrightColors !== boldBright) term.options.drawBoldTextInBrightColors = boldBright;
      updateGpu();
      const next = matcherForServer(serverId);
      if (next !== matcher && highlighter) {
        matcher = next;
        const held = highlighter.setMatcher(next, term.buffer.active.type === 'alternate');
        if (held) term.write(held);
      }
    },
    /** A shell on this computer (`local`) opens here. */
    openLocal: (syncSize: () => void, disabledRef: MutableRefObject<boolean>) => {
      if (!local) return;
      invoke<string>('local_terminal_open', {
        terminalId,
        start: { shell: local.shell, cwd: local.cwd ?? null },
        cols: term.cols || 80,
        rows: term.rows || 24,
        onOutput: fork.outputChannel(),
      }).then((label) => {
        syncSize();
        local.onOpened?.(label);
      }, e => {
        term.writeln(`\x1b[31mCouldn't start the shell: ${e}\x1b[0m`);
        // Nothing to type into: keys stop here (the tab marks it ended).
        disabledRef.current = true;
        local.onFailed?.();
      });
    },
    /** Last in the mount: the AI side pane and the activity mark know of it. */
    register: () => {
      term.textarea?.addEventListener('focus', onTextFocus);
      term.textarea?.addEventListener('blur', onTextBlur);
      // An AI tab's terminal, which the AI side pane shows beside a server tab;
      // and server terminals, which the pane gives the keyboard back to. Other
      // Home shells stay in Home, and a Wall tile's mirror is never moved.
      const dockable = !attachOnly && (!local || !!local.shell?.startsWith('ai:'));
      unregister = !dockable ? null : registerTerminal({
        terminalId,
        sessionId,
        term,
        host: dock,
        home,
        moved: (docked) => {
          fork.dockedRef.current = docked;
          setLooking(terminalId, fork.lookingNow());
          // Moving the element can take the keyboard from it without a blur
          // event, so xterm would go on drawing the cursor of a terminal being
          // typed in. Only then: xterm tells a program that asked (vim, tmux)
          // about each blur, and one that never had focus mustn't hear it.
          if (focused && term.textarea && document.activeElement !== term.textarea) {
            term.textarea.dispatchEvent(new FocusEvent('blur'));
          }
          // Refit, redraw and take or give up the GPU a few terminals per
          // frame, not all at once (terminalDock, #127).
          settleAfterMove(terminalId, () => {
            updateGpu();
            fitAddon.fit();
            term.refresh(0, Math.max(0, term.rows - 1));
          });
        },
      });
      unwatch = local || attachOnly ? null : watchTerminal(terminalId, sessionId);
      setLooking(terminalId, fork.lookingNow());
    },
    /** First in the cleanup: out of the AI side pane and the activity mark. */
    unmount: () => {
      unwatch?.();
      unregister?.();
      term.textarea?.removeEventListener('focus', onTextFocus);
      term.textarea?.removeEventListener('blur', onTextBlur);
      fork.stopReadyWatch();
      ptyDisposed = true;
    },
    /** Before xterm goes: its output and its GPU context. */
    dispose: () => {
      fork.outputRef.current = null;
      if (fromOwner) wallTileOutput.get(terminalId)?.delete(fromOwner);
      highlighter?.dispose();
      if (highlighter && fork.highlightRef.current === highlighter) fork.highlightRef.current = null;
      webgl?.dispose();
      releaseGpu(gpuSlot);
      fork.gpuUpdateRef.current = () => {};
      // TerminalView scrolls to the bottom two frames after a resize, and
      // keeps xtermRef after xterm has gone: one that opened and closed in
      // between (a Wall left while its tiles still open, #127) would throw.
      term.scrollToBottom = () => {};
    },
    /** After xterm has gone: the div it drew into, and terminalRef back to
     *  TerminalView's own. */
    undock: () => {
      dock.remove();
      if (fork.dockElRef.current === dock) fork.dockElRef.current = null;
      fork.setDockEl((cur) => (cur === dock ? null : cur));
      if (ref.current === host) ref.current = home;
      if (!attachOnly) forgetTerminal(terminalId);
    },
  };
};

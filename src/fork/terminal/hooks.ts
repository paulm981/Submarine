import { useEffect, useRef, useState } from 'react';
import type { MutableRefObject } from 'react';
import type { Terminal } from '@xterm/xterm';
import { Channel } from '@tauri-apps/api/core';
import type { HighlightStream } from '../../ui/highlight';
import { useRootInvoke } from '../../ui/rootInvoke';
import { setLooking } from '../../ui/terminalActivity';
import { LOCAL_COMMANDS, SHELL_FIRST_OUTPUT_MS, SHELL_READY_CAP_MS, SSH_COMMANDS } from './rules';

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state in a terminal, as one hook TerminalView calls in one line
// (#163): TerminalView.tsx is the original project's file, so every line the
// fork adds there can conflict at the next sync. It keeps what TerminalView
// kept, with the same effects, in the same order.
// ─────────────────────────────────────────────────────────────────────────────

/** The fork's props of a terminal, beside the original project's. */
export type TerminalProps = {
  /// This terminal's server is on screen (in front, or a tile of a split):
  /// with `shown`, you are looking at it, so it has no activity mark.
  /// Defaults to isActiveView.
  inView?: boolean;
  /// A shell on this computer (the Home view) instead of one on a server:
  /// `shell` is a list_local_shells id, or null for the default; `onOpened` gets
  /// the label of the shell that started. A local terminal is never pinned
  /// to the Wall or broadcast to, and keeps no command history — it isn't a
  /// server session. An AI tab's can be moved into the AI side pane.
  local?: { shell: string | null; cwd?: string; onOpened?: (label: string) => void; onFailed?: () => void };
  /// Called each time this terminal's SSH shell has been opened (first
  /// connect and every reconnect) and has printed what it prints on login,
  /// so typed-in commands land at its prompt (SessionView's run-on-connect).
  onShellReady?: () => void;
};

export const useTerminal = ({
  sessionId, terminalId, local, attachOnly, serverId, onShellReady, shown, shownRef, isActive, isActiveRef, isActiveView, inView,
}: TerminalProps & {
  sessionId: string;
  terminalId: string;
  attachOnly: boolean;
  serverId: number;
  shown: boolean;
  /// The original project's mirror of `shown`. The fork sets it in an
  /// effect, with the GPU (below), not while rendering.
  shownRef: MutableRefObject<boolean>;
  isActive: boolean;
  isActiveRef: MutableRefObject<boolean>;
  isActiveView: boolean;
}) => {
  const commands = local ? LOCAL_COMMANDS : SSH_COMMANDS;

  // What draws this terminal's output (set up with its xterm, mountTerminal).
  // Each open — the first and every reconnect — passes a fresh channel to the
  // backend that hands its messages to it.
  const outputRef = useRef<((data: Uint8Array) => void) | null>(null);
  // Syntax highlighting of the output (src/ui/highlight.ts).
  const highlightRef = useRef<HighlightStream | null>(null);
  const outputChannel = () => {
    const ch: Channel<ArrayBuffer> = new Channel<ArrayBuffer>((buf) => outputRef.current?.(new Uint8Array(buf)));
    return ch;
  };
  // A new shell: nothing the old one set (a full-screen program, colors)
  // carries over into highlighting.
  const resetHighlight = (term: Terminal) => {
    const held = highlightRef.current?.reset();
    if (held) term.write(held);
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

  // A container's terminal may need root (open_container_terminal).
  const rootInvoke = useRootInvoke();

  // What the AI side pane moves beside a server tab (terminalDock): the div
  // xterm draws into, and over it this terminal's search box, Saved password
  // button and toast, so they go with it. The ref is for long-lived handlers,
  // the state to render the overlays into it (TerminalOverlay).
  const dockElRef = useRef<HTMLDivElement | null>(null);
  const [dockEl, setDockEl] = useState<HTMLDivElement | null>(null);

  // Takes or gives up a GPU context as the terminal comes on screen or goes
  // (set with its xterm, mountTerminal).
  const gpuUpdateRef = useRef<() => void>(() => {});
  useEffect(() => {
    shownRef.current = shown;
    gpuUpdateRef.current();
  }, [shown, isActive]);
  // While the AI side pane shows this terminal (src/ui/terminalDock.ts) its
  // xterm sits in the pane, on screen whatever its own tab is doing — so it
  // refits to the pane even when it isn't the tab's active terminal.
  const dockedRef = useRef(false);
  // Activity mark (src/ui/terminalActivity.ts): looking = shown in a server
  // tab that is on screen, or in the AI side pane.
  const inViewRef = useRef(inView ?? isActiveView);
  const lookingNow = () => (shownRef.current && inViewRef.current) || dockedRef.current;
  useEffect(() => {
    inViewRef.current = inView ?? isActiveView;
    setLooking(terminalId, lookingNow());
  }, [terminalId, shown, isActive, isActiveView, inView]);
  // On screen in any way: the terminal being typed in, a pane of a split,
  // or in the AI side pane. Only then does it follow the size of its box.
  const onScreen = () => isActiveRef.current || shownRef.current || dockedRef.current;

  return {
    sessionId, terminalId, local, attachOnly, serverId, commands, outputRef, highlightRef, outputChannel, resetHighlight,
    readyWatchRef, stopReadyWatch, shellReady, watchForShellReady, rootInvoke, dockElRef, dockEl, setDockEl,
    gpuUpdateRef, dockedRef, lookingNow, onScreen,
  };
};

/** What useTerminal gives TerminalView, for the rest of the fork's code. */
export type TerminalFork = ReturnType<typeof useTerminal>;

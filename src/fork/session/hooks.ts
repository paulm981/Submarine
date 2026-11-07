import { useEffect, useRef, useState } from "react";
import type { CSSProperties, Dispatch, MutableRefObject, SetStateAction } from "react";
import { invoke } from "@tauri-apps/api/core";
import { skipIfKept } from "../../components/KeptSessions";
import { useIsNarrow } from "../../hooks/useViewport";
import { launchAiTool } from "../../ui/aiPane";
import type { ConfirmOptions } from "../../ui/confirm";
import { reorderById, useTabDrag } from "../../ui/tabDrag";
import { dockedTerminal } from "../../ui/terminalDock";
import { findAiTools, sharedWith, useAi } from "../../util/ai";
import { withoutPasteControls } from "../../util/pasteText";
import { IS_ANDROID } from "../../util/platform";
import { type AddLog, JUMP_GONE, type LogEntry, type SessionInfo, type SessionTerminal } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state in a server's tab, as hooks SessionView calls in one line
// each (#163): SessionView.tsx is the original project's file, so every line
// the fork adds there can conflict at the next sync. Each keeps what
// SessionView kept, with the same effects, called where SessionView had them.
// ─────────────────────────────────────────────────────────────────────────────

/** Refused before connecting because the server's jump host was deleted
 *  (src-tauri/src/jump_gone.rs): what to say (`note`), and the two choices
 *  (JumpGoneChoices). Connect directly takes the click: it clears the note
 *  (and a link to nothing) and connects; the host key is checked as always.
 *  Pick a jump host opens the server's form at Jump via (DesktopApp). */
export const useJumpGone = (serverId: number, reconnect: () => void, failed: (msg: string) => void) => {
  const [note, setNote] = useState<string | null>(null);
  return {
    note,
    /** A refused connect: a deleted jump host is noted here, and true (it
     *  has a box of its own); anything else is false, for the banner. */
    refused: (e: unknown): boolean => {
      const text = String(e);
      if (!text.startsWith(JUMP_GONE)) return false;
      setNote(text.slice(JUMP_GONE.length).trim());
      return true;
    },
    /** A new connect starts. */
    clear: () => setNote(null),
    connectDirectly: () => {
      invoke("jump_host_connect_directly", { serverId })
        .then(() => reconnect())
        .catch((e) => failed(`Couldn't change it: ${e}`));
    },
    pickJumpHost: () => {
      window.dispatchEvent(new CustomEvent("submarine-pick-jump-host", { detail: { serverId } }));
    },
  };
};
export type JumpGone = ReturnType<typeof useJumpGone>;

/** What get_servers says of a server, as far as the on-connect commands go. */
type HeldServer = { id: number; run_on_connect?: string | null; run_on_connect_every?: boolean; held?: string[] | null };

/** Per-node "run on connect" commands (servers.run_on_connect, e.g.
 *  `dzdo su -`): typed into the first terminal once its shell is up — on
 *  the first connect, and again after a reconnect, whose fresh login has
 *  lost what they did, as MobaXterm runs a session's command on each start.
 *  With the server's "each new terminal" (#256), every terminal of the tab
 *  but a container's gets them too. Typed only once the terminal's shell is
 *  open and has printed its login banner (TerminalView's onShellReady): typed
 *  any sooner, through a slow jump host, they reached a terminal that
 *  wasn't open yet and were dropped without a word. The original project's
 *  way (typed after the connect event) is gone from SessionView.
 *
 *  Gives each terminal's onShellReady, or undefined when it gets none. */
export const useOnConnect = ({ session, terminals, connectionEpoch, confirmRef, addLog }: {
  session: SessionInfo;
  terminals: SessionTerminal[];
  connectionEpoch: number;
  /** SessionView's useConfirm, through a ref: the shell-ready callback
   *  outlives renders. */
  confirmRef: MutableRefObject<(opts: ConfirmOptions | string) => Promise<boolean>>;
  addLog: AddLog;
}) => {
  // Once per terminal per connection: the terminal mounting again (into or
  // out of a split) doesn't type them a second time. By terminal id: the
  // connection they were typed for.
  const startupRanForEpochRef = useRef<Map<string, number>>(new Map());
  // When they are held (set by someone this profile is shared with), one
  // question per connection for all of the tab's terminals: the text it
  // allowed, or null.
  const onConnectAskRef = useRef<{ epoch: number; answer: Promise<string | null> } | null>(null);
  const startupTerminalId = `${session.id}-term-0`;
  const startupCommands = (session.runOnConnect || "")
    .split(/\r?\n/)
    .map((l: string) => l.trimEnd())
    .filter((l: string) => l.trim().length > 0);
  return (terminalId: string) => {
    const container = terminals.find(t => t.id === terminalId)?.container;
    const gets = terminalId === startupTerminalId || (!!session.runOnConnectEvery && !container);
    return gets && startupCommands.length > 0
      ? skipIfKept(terminalId, () => {
          if (startupRanForEpochRef.current.get(terminalId) === connectionEpoch) return;
          startupRanForEpochRef.current.set(terminalId, connectionEpoch);
          // Typed line by line, without escape sequences or other control
          // characters (a tab stays): the commands can come from anyone the
          // profile is shared with, and one could press keys nobody saw in
          // the text (pasteText.ts). The terminal says when any were left out.
          const typeThem = (text: string) => {
            const asSet = text.split(/\r\n|\r|\n/);
            const lines = asSet
              .map((l: string) => withoutPasteControls(l).trimEnd())
              .filter((l: string) => l.trim().length > 0);
            const cleaned = asSet.some((l: string) => withoutPasteControls(l) !== l);
            if (cleaned) {
              dockedTerminal(terminalId)?.term.write(
                '\r\n\x1b[33mSubmarine: control characters were left out of the on-connect commands.\x1b[0m\r\n',
              );
              addLog(`Control characters were left out of the on-connect commands for ${session.serverName}.`, "warn");
            }
            if (lines.length === 0) return;
            const bytes = Array.from(new TextEncoder().encode(lines.join('\n') + '\n'));
            invoke('write_terminal_data', { terminalId, data: bytes }).catch(() => {});
          };
          if (!session.runOnConnectHeld) {
            typeThem(session.runOnConnect || "");
            return;
          }
          // Someone this profile is shared with set or changed these
          // commands and this device hasn't allowed them: ask, never type
          // them unasked. Cancel has the focus, so a stray Enter is a no.
          // The OK counts only for what was shown (the text, and whether they
          // go into each new terminal): when the commands changed again
          // meanwhile, it's refused, and the new ones are shown. The answer is
          // the text allowed, or null.
          const ask = async (text: string, every: boolean): Promise<string | null> => {
            const ok = await confirmRef.current({
              title: "Run the on-connect commands?",
              message: `Someone you share this profile with set these commands to run when you connect to ${session.serverName}:\n\n${text.trimEnd()}\n\nThey would be typed into your terminal on that server${every ? ", and into each new terminal you open there" : ""}.`,
              okLabel: "Allow and run",
              cancelLabel: "Not now",
              safeDefault: true,
            });
            if (!ok) {
              addLog(`On-connect commands for ${session.serverName} not run: they are waiting for your OK.`, "info");
              return null;
            }
            try {
              await invoke("allow_held", { serverId: session.serverId, items: ["run_on_connect"], runOnConnect: text, runOnConnectEvery: every });
              window.dispatchEvent(new Event("submarine-held-changed"));
              return text;
            } catch (e) {
              if (!String(e).includes("HELD_CHANGED")) {
                addLog(`ALLOW_FAILED: ${e}`, "error");
                return null;
              }
              window.dispatchEvent(new Event("submarine-held-changed"));
              addLog(`The on-connect commands for ${session.serverName} were changed again. Look at them again before you allow them.`, "warn");
              let server: HeldServer | undefined;
              try {
                server = (await invoke<HeldServer[]>("get_servers")).find((s) => s.id === session.serverId);
              } catch (e2) {
                addLog(`ALLOW_FAILED: ${e2}`, "error");
                return null;
              }
              const now: string = server?.run_on_connect || "";
              if (!now.trim()) return null;
              // Still held: ask about the new text. Not held any more: it was
              // allowed meanwhile, or changed on this device.
              return (server?.held || []).includes("run_on_connect") ? ask(now, !!server?.run_on_connect_every) : now;
            }
          };
          let asked = onConnectAskRef.current;
          if (!asked || asked.epoch !== connectionEpoch) {
            asked = { epoch: connectionEpoch, answer: ask(session.runOnConnect || "", !!session.runOnConnectEvery) };
            onConnectAskRef.current = asked;
          }
          void asked.answer.then((text) => { if (text !== null) typeThem(text); });
        })
      : undefined;
  };
};

/** Dragging a terminal tab to a new place reorders `terminals` (the Wall
 *  picker, which reads them through onTerminalsChange, follows). The
 *  terminals stay mounted: only their order in the strip changes. */
export const useTermDrag = (sessionId: string, setTerminals: Dispatch<SetStateAction<SessionTerminal[]>>) =>
  useTabDrag(`terms-${sessionId}`, (dragId, overId, after) =>
    setTerminals(prev => reorderById(prev, dragId, overId, after)));
export type TermDrag = ReturnType<typeof useTermDrag>;

/** A terminal tab's own box: draggable (ui/tabDrag), dimmed while dragged,
 *  with a line on the side a dragged tab would land; its text can't be
 *  selected, so a drag doesn't select it. */
export const termTabProps = (drag: TermDrag, id: string): ReturnType<TermDrag["tabProps"]> & { className?: string; style?: CSSProperties } => {
  const mark = drag.tabMark(id);
  return { className: `group relative flex items-center select-none ${mark.dim ? 'opacity-50' : ''}`, ...drag.tabProps(id), style: mark.style };
};

/** Share with AI (#32): this tab, with one AI tab; the menu picks which
 *  when more than one is open. Stopping takes it from every AI tab.
 *  And the + menu's "Claude Code beside this" (#100): the AI tools Home's
 *  menu offers, looked for as the menu opens. Picked, it starts in Home, this
 *  tab is shared with it, and the AI side pane shows it beside this tab —
 *  offered only in a window as wide as DesktopApp shows that pane in. */
export const useShareWithAi = ({ session, displayName, addLog, plusMenu, closePlusMenu }: {
  session: SessionInfo;
  displayName?: string;
  addLog: AddLog;
  plusMenu: unknown;
  closePlusMenu: () => void;
}) => {
  const isNarrow = useIsNarrow();
  const ai = useAi();
  const aiSharedWith = sharedWith(ai, session.id);
  const [aiMenu, setAiMenu] = useState<{ x: number; y: number } | null>(null);
  const shareWithAi = (tabId: string) => {
    setAiMenu(null);
    invoke("ai_share", { tabId, sessionId: session.id, name: displayName ?? session.serverName })
      .catch((e) => addLog(`AI_SHARE: ${e}`, "error"));
  };
  const stopSharingWithAi = () => {
    invoke("ai_unshare", { sessionId: session.id }).catch((e) => addLog(`AI_UNSHARE: ${e}`, "error"));
  };
  const [aiTools, setAiTools] = useState<{ id: string; label: string }[]>([]);
  useEffect(() => {
    if (!plusMenu || !ai.enabled || IS_ANDROID || isNarrow) return;
    let live = true;
    void findAiTools().then((t) => { if (live) setAiTools(t); });
    return () => { live = false; };
  }, [plusMenu, ai.enabled, isNarrow]);
  const startAiBeside = (tool: { id: string; label: string }) => {
    closePlusMenu();
    const name = displayName ?? session.serverName;
    launchAiTool(tool.id, tool.label, session.id, (tabId) => {
      invoke("ai_share", { tabId, sessionId: session.id, name })
        .catch((e) => addLog(`AI_SHARE: ${e}`, "error"));
    });
  };
  return {
    ai, aiSharedWith, aiMenu, setAiMenu, shareWithAi, stopSharingWithAi, startAiBeside,
    /** The AI tools the + menu offers beside this tab (none where it can't). */
    besideTools: ai.enabled && !IS_ANDROID && !isNarrow ? aiTools : [],
  };
};
export type ShareWithAi = ReturnType<typeof useShareWithAi>;

/** A connect failed and will be tried again: say what got through (name,
 *  port, jump host) under the error. It runs beside the countdown, and is
 *  dropped if that moved on (the next attempt started, or Cancel) before the
 *  answer came back. SessionView clears `timerRef` when the countdown fires
 *  for this. Called right after the retry is scheduled. */
export const diagnoseConnect = (
  session: SessionInfo,
  attemptRef: MutableRefObject<number>,
  timerRef: MutableRefObject<unknown>,
  pushLog: (entry: LogEntry) => void,
) => {
  const waiting = attemptRef.current;
  // A Quick Connect target goes without its password or key: the check
  // only needs where it goes (and the user, for ~/.ssh/config).
  const q = session.quickAuth;
  const quickTarget = q ? { host: q.host, port: q.port, username: q.username } : null;
  invoke<string[]>("diagnose_connect", { serverId: session.serverId, quickTarget })
    .then((lines) => {
      if (attemptRef.current !== waiting || !timerRef.current) return;
      lines.forEach((msg) => pushLog({ msg, type: 'info' }));
    })
    .catch(() => {});
};

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import {
  ClipboardPaste, ExternalLink, LayoutGrid, Pause, Play, Plus, Radio, RotateCw, Save, Send, Ungroup, X,
} from "lucide-react";
import {
  appHotkey, claimTerminal, DockEntry, dockedTerminal, forCursorMode, markTyped, terminalMark, useDockVersion,
} from "../ui/terminalDock";
import { lineAfter, TYPED_KEPT } from "../ui/wallLine";
import { useConfirm } from "../ui/confirm";
import { sessionDot } from "../util/sessionStatus";
import { IS_LINUX } from "../util/platform";
import { readClipboardText } from "../util/clipboard";

// ─────────────────────────────────────────────────────────────────────────────
// A Wall: terminals from many servers side by side, MobaXterm MultiExec style.
//
// The tiles show the terminals themselves (moved in from their tabs while the
// Wall is on screen, see src/ui/terminalDock.ts). With Broadcast on:
//   • typing goes to every terminal that can take it — a hidden terminal of
//     the Wall's own holds the keyboard and hands each key to all of them;
//   • clicking into one tile types to that one only (to fix one, stop one,
//     answer one); "Type to all" on the bar, the ✕ on that tile's "Only this
//     one", or a click on the bar or between the tiles types to all again;
//   • a paused terminal (its chip, or Ctrl+Shift+P) is left out until it's
//     resumed; one that isn't connected drops out, and is back in as soon as
//     it reconnects — at the start of a line: one that starts taking input
//     partway through a line joins at the next (see wallLines);
//   • a paste — or Send, for a set of commands — goes to all, after saying
//     how many lines into how many servers.
// ─────────────────────────────────────────────────────────────────────────────

export interface WallMember { sessionId: string; terminalId: string; }

export interface Wall {
  id: string;
  name: string;
  /// In tile order.
  members: WallMember[];
  broadcast: boolean;
  /// Terminal ids left out of the broadcast.
  paused: string[];
  /// With Broadcast on, the one terminal being typed to (clicked into);
  /// null = typing to all. Kept here so leaving the Wall and coming back
  /// doesn't quietly switch to typing to all.
  solo: string | null;
  /// Tiles per row; 0 = fit to the count.
  cols: number;
  /// The saved wall it was opened from or saved as.
  savedId: number | null;
  /// Broadcast takes the open tabs into its Wall: the sessions that were
  /// tabs then, so "Back to tabs" and closing the Wall can put them back
  /// (and tell them from sessions opened into the Wall itself). Their tab
  /// order is just their order in the session list, which isn't touched.
  fromTabs?: string[];
}

export interface WallSession { id: string; serverName: string; wallId?: string; }
export interface Snippet { id: number; title: string; content: string; }

const autoCols = (n: number) => (n <= 1 ? 1 : n <= 4 ? 2 : n <= 9 ? 3 : n <= 16 ? 4 : n <= 30 ? 5 : 6);

const lineCount = (text: string) => text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n").length;
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const encode = (s: string) => Array.from(new TextEncoder().encode(s));

// ── The line being typed to all ──────────────────────────────────────────────
// A terminal that starts taking input partway through a line — it has just
// connected (behind a jump host they log in a few at a time), reconnected,
// been resumed, or was typed into some other way — would get only the rest
// of it: `rm -rf *` without the `cd /old/site && ` typed before it. So a line
// goes on only to the terminals that have had all of it so far (their
// terminalMark unchanged since); the others join at the next one. Kept per
// wall (Wall.id) out here, so leaving the Wall or turning Broadcast off and
// on again doesn't forget a line half typed.
interface WallLine {
  /// terminalId → its terminalMark after it was last typed to here.
  members: Map<string, number>;
  /// What was typed of it, as far as can be followed (lineAfter).
  typed: string;
}
const wallLines = new Map<string, WallLine>();
const sameMembers = (a: Map<string, number>, b: Map<string, number>) =>
  a.size === b.size && [...a].every(([tid, shell]) => b.get(tid) === shell);

// What a confirm shows of the text about to go out.
const preview = (text: string) => {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n");
  const shown = lines.slice(0, 6).map((l) => (l.length > 100 ? `${l.slice(0, 100)}…` : l));
  if (lines.length > 6) shown.push(`… and ${plural(lines.length - 6, "more line")}`);
  return shown.join("\n");
};

export const WallView = ({
  wall, sessions, statuses, waiting, terminalTitles, snippets,
  onChange, onOpenSession, onRemove, onReconnect, onAdd, onSave, onRename, onBackToTabs,
}: {
  wall: Wall;
  sessions: WallSession[];
  statuses: Record<string, string>;
  waiting: Set<string>;
  terminalTitles: Record<string, { id: string; title: string }[]>;
  snippets: Snippet[];
  onChange: (patch: Partial<Wall>) => void;
  /// Show the session in a tab of its own.
  onOpenSession: (sessionId: string) => void;
  /// Off the Wall: a session opened for it closes, any other keeps its tab.
  onRemove: (m: WallMember) => void;
  onReconnect: (sessionId: string) => void;
  onAdd: () => void;
  onSave: () => void;
  onRename: () => void;
  /// Put the sessions back as tabs and close the Wall (they stay connected).
  onBackToTabs: () => void;
}) => {
  useDockVersion();
  const confirm = useConfirm();
  // The keyboard is nowhere: the tile being typed to left the Wall. Typing
  // to all again takes a click — it never just starts. `soloGone` is that
  // moment, before `parked` is set.
  const soloGone = !!wall.solo && !wall.members.some((m) => m.terminalId === wall.solo);
  const solo = soloGone ? null : wall.solo;
  const setSolo = (tid: string | null) => { if (tid !== wall.solo) onChange({ solo: tid }); };
  const [parkedState, setParked] = useState(false);
  const parked = parkedState || soloGone;
  const [sendOpen, setSendOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const noteTimer = useRef(0);
  const flash = (msg: string) => {
    setNote(msg);
    window.clearTimeout(noteTimer.current);
    noteTimer.current = window.setTimeout(() => setNote(null), 3000);
  };
  useEffect(() => () => window.clearTimeout(noteTimer.current), []);

  // Read by the keyboard terminal's long-lived handlers.
  const wallRef = useRef(wall);
  wallRef.current = wall;
  const soloRef = useRef(solo);
  soloRef.current = solo;
  const hoveredRef = useRef<string | null>(null);

  const paused = new Set(wall.paused);
  const isPaused = (tid: string) => paused.has(tid);

  // Who a broadcast reaches right now: not paused, and taking input.
  const targetsNow = useCallback(() => {
    const p = new Set(wallRef.current.paused);
    return wallRef.current.members
      .filter((m) => !p.has(m.terminalId))
      .map((m) => dockedTerminal(m.terminalId))
      .filter((e): e is NonNullable<typeof e> => !!e && e.live());
  }, []);
  // ── The line being typed (wallLines) ─────────────────────────────────────
  // Shown again when who has the line, or whether one is open, changes.
  const [, setLineShown] = useState(0);
  const lineChanged = useCallback(() => setLineShown((v) => v + 1), []);
  // Who typing reaches: those taking input that have had all of the line.
  const lineTargets = useCallback(() => {
    const now = targetsNow();
    const line = wallLines.get(wallRef.current.id);
    return line ? now.filter((e) => line.members.get(e.terminalId) === terminalMark(e.terminalId)) : now;
  }, [targetsNow]);
  /// `data` went to `sent` (none, when nobody was taking input: a line
  /// begun then goes on to nobody): a line begins with them, goes on with
  /// them, or has ended. A paste (`pasted`) leaves the line open — a shell
  /// that asked for pastes bracketed holds the text, line breaks and all,
  /// until Enter.
  const afterTyping = useCallback((data: string, sent: DockEntry[], pasted = false) => {
    const id = wallRef.current.id;
    const was = wallLines.get(id);
    const typed = pasted ? ((was?.typed ?? "") + data).slice(-TYPED_KEPT) : lineAfter(was?.typed ?? null, data);
    if (typed === null) {
      if (was) {
        wallLines.delete(id);
        lineChanged();
      }
      return;
    }
    const members = new Map(sent.map((e) => [e.terminalId, terminalMark(e.terminalId)]));
    wallLines.set(id, { members, typed });
    if (!was || !sameMembers(was.members, members)) lineChanged();
  }, [lineChanged]);
  const line = wallLines.get(wall.id);
  const atNextLine = (tid: string) => {
    const e = dockedTerminal(tid);
    return !!line && !!e && line.members.get(tid) !== terminalMark(tid);
  };

  const live = wall.members.filter((m) => dockedTerminal(m.terminalId)?.live());
  const receiving = live.filter((m) => !isPaused(m.terminalId) && !atNextLine(m.terminalId));
  const leftOut = (() => {
    const p = wall.members.filter((m) => isPaused(m.terminalId)).length;
    const off = wall.members.length - live.length;
    const next = live.filter((m) => !isPaused(m.terminalId) && atNextLine(m.terminalId)).length;
    const parts = [];
    if (p) parts.push(`${p} paused`);
    if (off) parts.push(`${off} not connected`);
    if (next) parts.push(`${next} at the next line`);
    return parts.join(" · ");
  })();

  // ── The keyboard terminal ────────────────────────────────────────────────
  // Typing to all goes through a terminal nobody sees: it turns keys into
  // what a terminal sends (arrows, Ctrl+C, function keys…) exactly as a
  // tile's own would, and each key is written to every target in one call.
  const ghostBox = useRef<HTMLDivElement>(null);
  const ghostRef = useRef<Terminal | null>(null);
  const focusGhost = useCallback(() => {
    requestAnimationFrame(() => {
      if (document.querySelector('[aria-modal="true"]')) return;
      ghostRef.current?.focus();
    });
  }, []);

  const sendKeys = useCallback((data: string) => {
    if (!wallRef.current.broadcast || soloRef.current) return;
    const groups = new Map<string, string[]>();
    const sent = lineTargets();
    for (const e of sent) {
      const d = forCursorMode(data, e.term.modes.applicationCursorKeysMode);
      const ids = groups.get(d);
      if (ids) ids.push(e.terminalId);
      else groups.set(d, [e.terminalId]);
    }
    for (const [d, ids] of groups) {
      markTyped(...ids);
      invoke("write_terminals_data", { terminalIds: ids, data: encode(d) }).catch(() => {});
    }
    afterTyping(data, sent);
  }, [lineTargets, afterTyping]);

  // A paste into all of them goes through each terminal's own paste, so each
  // gets it the way pasting into it alone would (bracketed for vim and
  // modern shells). Anything with a line break runs something, so it asks.
  // It goes only to the servers the confirm named (those still taking input
  // when it's answered), and not at all if Broadcast went off meanwhile.
  const pasteToAll = useCallback(async (text: string) => {
    if (!text || !wallRef.current.broadcast) return;
    const first = lineTargets();
    if (first.length === 0) {
      flash("No terminal is taking input.");
      return;
    }
    const named = new Set(first.map((e) => e.terminalId));
    if (/[\r\n]/.test(text)) {
      const skipped = wallRef.current.members.length - first.length;
      const ok = await confirm({
        title: `Paste ${plural(lineCount(text), "line")} into ${plural(first.length, "server")}?`,
        message: preview(text) + (skipped ? `\n\n${plural(skipped, "terminal")} paused, not connected or joining at the next line won't get it.` : ""),
        okLabel: "Paste",
      });
      focusGhost();
      if (!ok || !wallRef.current.broadcast) return;
    }
    const sent = lineTargets().filter((e) => named.has(e.terminalId) && e.paste(text));
    const n = sent.length;
    afterTyping(text.replace(/\r\n?/g, "\n"), sent, true);
    flash(n === named.size ? `Pasted into ${plural(n, "server")}.` : `Pasted into ${n} of ${plural(named.size, "server")} — the rest stopped taking input.`);
  }, [confirm, focusGhost, lineTargets, afterTyping]);
  const pasteToAllRef = useRef(pasteToAll);
  pasteToAllRef.current = pasteToAll;

  const pasteClipboardToAll = () => {
    readClipboardText().then((t) => pasteToAllRef.current(t), (e) => flash(typeof e === "string" ? e : "Couldn't read the clipboard."));
  };
  const pasteClipboardRef = useRef(pasteClipboardToAll);
  pasteClipboardRef.current = pasteClipboardToAll;

  useEffect(() => {
    const box = ghostBox.current;
    if (!box) return;
    const term = new Terminal({ cols: 80, rows: 2, allowProposedApi: true });
    term.open(box);
    ghostRef.current = term;
    // The app's shortcuts pass by; on Linux the paste keys read the
    // clipboard through the backend, as a tile's own do (see TerminalView).
    term.attachCustomKeyEventHandler((ev) => {
      if (appHotkey(ev)) return false;
      const isPasteKey = IS_LINUX && !ev.altKey && !ev.metaKey && (
        (ev.ctrlKey && ev.shiftKey && ev.code === "KeyV") ||
        (ev.shiftKey && !ev.ctrlKey && ev.key === "Insert")
      );
      if (!isPasteKey) return true;
      ev.preventDefault();
      if (ev.type === "keydown" && !ev.repeat) pasteClipboardRef.current();
      return false;
    });
    const onData = term.onData(sendKeys);
    // A paste the webview delivers itself (Ctrl+Shift+V, ⌘V) is caught here,
    // before the hidden terminal would type it out unasked.
    const onPaste = (e: ClipboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      void pasteToAllRef.current(e.clipboardData?.getData("text/plain") ?? "");
    };
    box.addEventListener("paste", onPaste, true);
    return () => {
      box.removeEventListener("paste", onPaste, true);
      onData.dispose();
      ghostRef.current = null;
      term.dispose();
    };
  }, [sendKeys]);

  // Broadcast on and no tile picked: the keyboard goes to all. A tile
  // picked before the Wall was left gets the keyboard back.
  useEffect(() => {
    if (!wall.broadcast) {
      if (wall.solo) onChange({ solo: null });
      setParked(false);
      return;
    }
    if (solo) {
      const id = requestAnimationFrame(() => {
        if (!document.querySelector('[aria-modal="true"]')) dockedTerminal(solo)?.term.focus();
      });
      return () => cancelAnimationFrame(id);
    }
    if (!parked) focusGhost();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wall.broadcast, solo, parked, focusGhost]);

  // The tile being typed to left the Wall (closed, removed): the keyboard
  // goes nowhere rather than to all of them — nor stays with that terminal,
  // which keeps it when it goes back to its tab out of sight.
  useEffect(() => {
    if (!soloGone || !wall.solo) return;
    dockedTerminal(wall.solo)?.term.blur();
    setParked(true);
    onChange({ solo: null });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [soloGone]);

  const backToAll = () => {
    if (!wall.broadcast) return;
    setParked(false);
    setSolo(null);
    focusGhost();
  };

  const togglePause = (tid: string) => {
    const next = isPaused(tid) ? wall.paused.filter((t) => t !== tid) : [...wall.paused, tid];
    onChange({ paused: next });
  };
  const togglePauseRef = useRef(togglePause);
  togglePauseRef.current = togglePause;

  // Terminals picked to pause or resume together: Ctrl-click (⌘ on a Mac) a
  // title to add or drop one, Shift-click to take the run from the last one
  // picked, in tile order — as the session tree picks servers. Kept here, not
  // on the Wall: a pick is for the next button press, not something to keep.
  const [picked, setPicked] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);
  const pickedNow = picked.filter((t) => wall.members.some((m) => m.terminalId === t));
  const pick = (tid: string, e: React.MouseEvent) => {
    const order = wall.members.map((m) => m.terminalId);
    const from = anchor.current ? order.indexOf(anchor.current) : -1;
    if (e.shiftKey && from >= 0) {
      const to = order.indexOf(tid);
      const run = order.slice(Math.min(from, to), Math.max(from, to) + 1);
      setPicked((prev) => [...new Set([...prev, ...run])]);
      return;
    }
    anchor.current = tid;
    setPicked((prev) => (prev.includes(tid) ? prev.filter((t) => t !== tid) : [...prev, tid]));
  };
  const setPausedFor = (tids: string[], pause: boolean) => {
    const next = pause
      ? [...new Set([...wall.paused, ...tids])]
      : wall.paused.filter((t) => !tids.includes(t));
    onChange({ paused: next });
    setPicked([]);
  };

  // Ctrl+Shift+P pauses (or resumes) the tile being typed to, else the one
  // under the mouse.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (appHotkey(e) !== "pause" || e.repeat) return;
      e.preventDefault();
      if (document.querySelector('[aria-modal="true"]')) return;
      if (!wallRef.current.broadcast) {
        flash("Pausing is for Broadcast — turn it on first.");
        return;
      }
      const tid = soloRef.current ?? hoveredRef.current;
      if (!tid) {
        flash("Click a terminal (or point at it), then press Ctrl+Shift+P.");
        return;
      }
      togglePauseRef.current(tid);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const sendToAll = (text: string, run: boolean) => {
    let body = text.replace(/\r\n?/g, "\n");
    if (run && !body.endsWith("\n")) body += "\n";
    // Into a line half typed, only to those that have the start of it.
    const sent = lineTargets();
    if (sent.length === 0) {
      flash("No terminal is taking input.");
      return;
    }
    const data = body.replace(/\n/g, "\r");
    const ids = sent.map((e) => e.terminalId);
    markTyped(...ids);
    invoke("write_terminals_data", { terminalIds: ids, data: encode(data) })
      .then(() => flash(`Sent to ${plural(sent.length, "server")}.`), (e) => flash(`Couldn't send: ${e}`));
    afterTyping(data, sent);
  };

  const cols = wall.cols || autoCols(wall.members.length);
  const sessionOf = (sid: string) => sessions.find((s) => s.id === sid);
  const soloName = solo ? sessionOf(wall.members.find((m) => m.terminalId === solo)?.sessionId ?? "")?.serverName : null;
  // A click on the bar (not on its buttons) or between tiles: back to typing
  // to all. The buttons leave the keyboard where it was — clicking Cols or
  // Save while typing to one server mustn't make the next command go to all
  // of them. Send and Paste are for all, and say so themselves.
  const onBarMouseDown = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button, input, select, textarea")) {
      e.preventDefault();
      return;
    }
    backToAll();
  };
  const btn = "h-7 px-2.5 rounded-md text-[10px] font-bold uppercase tracking-wider flex items-center gap-1.5 border transition-colors";

  return (
    <div className="wall-root absolute inset-0 flex flex-col overflow-hidden bg-[#0a0a0d]">
      <div
        className="shrink-0 border-b border-white/5 bg-[#111114] flex flex-wrap items-center gap-2 px-3 py-1.5 min-h-10 relative"
        onMouseDown={onBarMouseDown}
      >
        {/* The keyboard terminal: in a 1px box so focusing it can't scroll
            anything on screen. */}
        <div aria-hidden="true" className="absolute left-0 top-0 w-px h-px overflow-hidden opacity-0 pointer-events-none">
          <div ref={ghostBox} style={{ width: 320, height: 60 }} />
        </div>
        <LayoutGrid size={12} className="text-primary/70 shrink-0" />
        <button
          onClick={onRename}
          title="Rename this wall"
          className="text-[11px] font-bold text-zinc-200 hover:text-white truncate max-w-[220px]"
        >
          {wall.name}
        </button>
        <span className="text-[10px] text-zinc-500 shrink-0">{plural(wall.members.length, "terminal")}</span>

        <button
          onClick={() => onChange({ broadcast: !wall.broadcast })}
          title={wall.broadcast ? "Stop typing to all of them (Ctrl+Shift+B)" : "Type to all of them at once (Ctrl+Shift+B)"}
          className={`${btn} ml-1 ${
            wall.broadcast
              ? "bg-orange-500/15 border-orange-400/40 text-orange-300"
              : "bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white"
          }`}
        >
          <Radio size={11} className={wall.broadcast ? "animate-pulse" : ""} />
          Broadcast {wall.broadcast ? "on" : "off"}
        </button>

        {wall.broadcast && (
          parked || solo ? (
            <>
              <span className="text-[10.5px] font-semibold text-primary truncate max-w-[260px]">
                {solo ? `Typing to ${soloName ?? "one terminal"} only` : "Not typing anywhere"}
              </span>
              <button
                onClick={backToAll}
                title="Type to all of them again"
                className={`${btn} bg-primary/15 border-primary/40 text-primary hover:bg-primary/25 hover:text-white`}
              >
                <Radio size={11} /> Type to all
              </button>
            </>
          ) : (
            <span className="text-[10.5px] font-semibold text-orange-300/90 shrink-0" title={leftOut || undefined}>
              Typing to all · {receiving.length} of {wall.members.length}
              {leftOut && <span className="text-zinc-500 font-normal"> ({leftOut})</span>}
            </span>
          )
        )}

        {wall.broadcast && pickedNow.length > 0 ? (
          <>
            <span className="text-[10.5px] font-semibold text-sky-300 shrink-0">{pickedNow.length} picked</span>
            <button
              onClick={() => setPausedFor(pickedNow, true)}
              title="Leave the picked terminals out of the broadcast"
              className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
            >
              <Pause size={11} /> Pause {pickedNow.length}
            </button>
            <button
              onClick={() => setPausedFor(pickedNow, false)}
              title="Let the picked terminals take the broadcast again"
              className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
            >
              <Play size={11} /> Resume {pickedNow.length}
            </button>
            <button
              onClick={() => setPicked([])}
              title="Pick none"
              className={`${btn} bg-white/[0.04] border-white/10 text-zinc-400 hover:bg-white/[0.08] hover:text-white`}
            >
              <X size={11} /> Clear
            </button>
          </>
        ) : wall.broadcast && (
          <>
            {wall.paused.length < wall.members.length && (
              <button
                onClick={() => setPausedFor(wall.members.map((m) => m.terminalId), true)}
                title="Leave every terminal out, then resume the ones you want. Ctrl-click (⌘-click) titles to pick several, Shift-click for a run."
                className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
              >
                <Pause size={11} /> Pause all
              </button>
            )}
            {wall.paused.length > 0 && (
              <button
                onClick={() => onChange({ paused: [] })}
                className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
              >
                <Play size={11} /> Resume all
              </button>
            )}
          </>
        )}
        {wall.broadcast && (
          <>
            <button
              onClick={() => { backToAll(); setSendOpen(true); }}
              title="Type a set of commands into all of them"
              className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
            >
              <Send size={11} /> Send…
            </button>
            <button
              onClick={() => { backToAll(); pasteClipboardToAll(); }}
              title="Paste the clipboard into all of them (or right-click a terminal)"
              className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
            >
              <ClipboardPaste size={11} /> Paste
            </button>
          </>
        )}

        <div className="flex-1" />
        <div className="flex items-center gap-1 shrink-0">
          <span className="text-[9px] font-bold uppercase tracking-wider text-zinc-500">Cols</span>
          {[0, 1, 2, 3, 4, 5, 6].map((n) => (
            <button
              key={n}
              onClick={() => onChange({ cols: n })}
              title={n === 0 ? "Fit to the number of terminals" : `${plural(n, "column")} per row`}
              className={`h-6 ${n === 0 ? "px-1.5" : "w-6"} rounded flex items-center justify-center text-[10px] font-bold transition-all ${
                wall.cols === n
                  ? "bg-primary/20 text-primary border border-primary/40"
                  : "text-zinc-400 hover:bg-white/[0.06] border border-transparent"
              }`}
            >
              {n === 0 ? "Auto" : n}
            </button>
          ))}
        </div>
        {/* Only while a server Broadcast took from the tabs is still here:
            one that went back to its tab (its ✕, opened on its own, closed)
            no longer counts. */}
        {sessions.some((s) => s.wallId === wall.id && wall.fromTabs?.includes(s.id)) && (
          <button
            onClick={onBackToTabs}
            title="Put the servers back as tabs and close this wall — they stay connected"
            className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
          >
            <Ungroup size={11} /> Back to tabs
          </button>
        )}
        <button onClick={onAdd} title="Add terminals to this wall" className={`${btn} bg-primary/15 hover:bg-primary/25 text-primary border-primary/30`}>
          <Plus size={11} /> Add
        </button>
        <button
          onClick={onSave}
          title={wall.savedId ? "Save changes to the saved wall" : "Save as a wall you can open again later"}
          className={`${btn} bg-white/[0.04] border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white`}
        >
          <Save size={11} /> Save
        </button>
      </div>

      <div
        className="flex-1 relative bg-black/40 overflow-auto custom-scrollbar"
        onMouseDown={(e) => { if (e.target === e.currentTarget || (e.target as HTMLElement).dataset.wallGrid) backToAll(); }}
      >
        {wall.members.length === 0 ? (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-4 text-center px-6 pointer-events-none">
            <LayoutGrid size={40} className="text-zinc-600" />
            <div className="text-zinc-400 text-[13px] max-w-md leading-relaxed">
              A wall shows terminals from many servers side by side. Use <span className="text-primary font-semibold">Add</span>,
              or open a folder of the session tree as a wall.
            </div>
          </div>
        ) : (
          <div
            data-wall-grid="1"
            className="min-h-full grid gap-1.5 p-1.5"
            style={{
              gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              gridAutoRows: wall.members.length <= 4 ? "minmax(220px, 1fr)" : "minmax(200px, 1fr)",
            }}
          >
            {wall.members.map((m) => {
              const s = sessionOf(m.sessionId);
              const terms = terminalTitles[m.sessionId] ?? [];
              return (
                <WallTile
                  key={m.terminalId}
                  member={m}
                  name={s?.serverName ?? m.sessionId}
                  closes={s?.wallId === wall.id && !wall.members.some((o) => o.sessionId === m.sessionId && o.terminalId !== m.terminalId)}
                  termTitle={terms.length > 1 ? terms.find((t) => t.id === m.terminalId)?.title : undefined}
                  status={statuses[m.sessionId] ?? "connecting"}
                  waiting={waiting.has(m.sessionId)}
                  broadcast={wall.broadcast}
                  solo={solo}
                  paused={isPaused(m.terminalId)}
                  picked={wall.broadcast && pickedNow.includes(m.terminalId)}
                  onPick={wall.broadcast ? (e) => pick(m.terminalId, e) : undefined}
                  nextLine={atNextLine(m.terminalId)}
                  onFocusIn={() => {
                    if (!wallRef.current.broadcast) return;
                    setParked(false);
                    setSolo(m.terminalId);
                  }}
                  onBackToAll={backToAll}
                  onHover={(on) => { hoveredRef.current = on ? m.terminalId : (hoveredRef.current === m.terminalId ? null : hoveredRef.current); }}
                  onPasteToAll={pasteClipboardToAll}
                  onTogglePause={() => togglePause(m.terminalId)}
                  onOpen={() => onOpenSession(m.sessionId)}
                  onRemove={() => onRemove(m)}
                  onReconnect={() => onReconnect(m.sessionId)}
                />
              );
            })}
          </div>
        )}
        {note && (
          <div className="sticky bottom-3 mx-auto w-fit px-3 py-1.5 rounded-lg bg-zinc-900/95 border border-white/10 text-[11px] text-zinc-200 shadow-xl pointer-events-none">
            {note}
          </div>
        )}
      </div>

      {sendOpen && (
        <SendToAllModal
          targets={() => lineTargets().length}
          total={wall.members.length}
          snippets={snippets}
          onSend={(text, run) => { setSendOpen(false); sendToAll(text, run); focusGhost(); }}
          onClose={() => { setSendOpen(false); focusGhost(); }}
        />
      )}
    </div>
  );
};

// One terminal on the Wall. Its body is the slot the terminal moves into
// (claimTerminal); while there's no terminal yet — still connecting, or
// waiting on a prompt — the slot is covered by what the session is doing.
const WallTile = ({
  member, name, closes, termTitle, status, waiting, broadcast, solo, paused, picked, onPick, nextLine,
  onFocusIn, onBackToAll, onHover, onPasteToAll, onTogglePause, onOpen, onRemove, onReconnect,
}: {
  member: WallMember;
  name: string;
  /// The ✕ closes the session (it was opened for this Wall).
  closes: boolean;
  termTitle?: string;
  status: string;
  waiting: boolean;
  broadcast: boolean;
  solo: string | null;
  paused: boolean;
  /// Picked, to pause or resume with others.
  picked: boolean;
  /// A Ctrl-, ⌘- or Shift-click on the title (Broadcast on only).
  onPick?: (e: React.MouseEvent) => void;
  /// Taking input, but joins the broadcast at the next line (lineTargets).
  nextLine: boolean;
  onFocusIn: () => void;
  onBackToAll: () => void;
  onHover: (on: boolean) => void;
  onPasteToAll: () => void;
  onTogglePause: () => void;
  onOpen: () => void;
  onRemove: () => void;
  onReconnect: () => void;
}) => {
  const slotRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const slot = slotRef.current;
    if (!slot) return;
    return claimTerminal(member.terminalId, slot);
  }, [member.terminalId]);

  const entry = dockedTerminal(member.terminalId);
  const isLive = !!entry?.live();
  const isSolo = solo === member.terminalId;
  const joinsNext = broadcast && !solo && isLive && !paused && nextLine;
  const receiving = broadcast && !solo && isLive && !paused && !nextLine;
  const dot = sessionDot(status, waiting);
  const border =
    isSolo ? "border-primary shadow-[0_0_0_1px_rgb(var(--primary)/0.5)]" :
    broadcast && paused ? "border-dashed border-zinc-600" :
    joinsNext ? "border-dashed border-orange-400/40" :
    receiving ? "border-orange-400/50" :
    "border-white/10";

  const headBtn = "h-5 w-5 rounded flex items-center justify-center text-zinc-500 hover:bg-white/[0.06] transition-all";
  return (
    <div
      className={`flex flex-col overflow-hidden border rounded-md bg-[#09090b] shadow-[0_2px_10px_rgba(0,0,0,0.4)] transition-colors ${border}`}
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
    >
      <div
        className={`h-7 shrink-0 flex items-center gap-2 px-2 border-b border-white/5 text-[10.5px] select-none ${picked ? "bg-sky-500/20" : "bg-[#141418]"}`}
        // Header buttons leave the keyboard where it was.
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          if (!onPick || !(e.ctrlKey || e.metaKey || e.shiftKey)) return;
          if ((e.target as HTMLElement).closest("button")) return;
          onPick(e);
        }}
        title={onPick ? "Ctrl-click (⌘-click) to pick several, Shift-click for a run" : undefined}
      >
        <span title={dot.title} className={`w-1.5 h-1.5 rounded-full shrink-0 ${dot.tone}`} />
        <span className="font-bold text-zinc-200 truncate" title={name}>{name}</span>
        {termTitle && <span className="text-zinc-500 truncate">· {termTitle}</span>}
        {isSolo && (
          <button
            onClick={onBackToAll}
            title="Type to all of them again"
            className="shrink-0 h-5 pl-1.5 pr-1 rounded flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider text-primary bg-primary/10 border border-primary/30 hover:bg-primary/20 hover:text-white transition-colors"
          >
            Only this one <X size={10} />
          </button>
        )}
        {joinsNext && (
          <span
            title="It started taking input partway through the line being typed, so it gets the next line (after Enter, or Ctrl+C), not the rest of this one."
            className="shrink-0 h-5 px-1.5 rounded flex items-center text-[9px] font-bold uppercase tracking-wider text-orange-300/90 border border-orange-400/30"
          >
            Next line
          </span>
        )}
        <div className="flex-1" />
        {broadcast && (
          <button
            onClick={onTogglePause}
            title={paused ? "Resume: take the broadcast again (Ctrl+Shift+P)" : "Pause: leave this one out of the broadcast (Ctrl+Shift+P)"}
            className={`h-5 px-1.5 rounded flex items-center gap-1 text-[9px] font-bold uppercase tracking-wider border transition-all ${
              paused
                ? "bg-zinc-700/40 border-zinc-500/50 text-zinc-200"
                : "border-transparent text-zinc-500 hover:text-zinc-200 hover:bg-white/[0.06]"
            }`}
          >
            {paused ? <><Play size={9} /> Paused</> : <Pause size={9} />}
          </button>
        )}
        <button onClick={onOpen} title="Open in its own tab" className={`${headBtn} hover:text-primary`}>
          <ExternalLink size={11} />
        </button>
        <button onClick={onRemove} title={closes ? "Close this session" : "Remove from this wall (it keeps its tab)"} className={`${headBtn} hover:text-red-400`}>
          <X size={11} />
        </button>
      </div>
      <div
        className="flex-1 min-h-0 relative"
        onFocusCapture={(e) => { if (slotRef.current?.contains(e.target as Node)) onFocusIn(); }}
        // Typing to all, only a left click picks this one: a right click
        // (paste to all) or a middle click mustn't move the keyboard here
        // first, so they never reach the terminal.
        onMouseDownCapture={(e) => {
          if (broadcast && solo === null && e.button !== 0) {
            e.preventDefault();
            e.stopPropagation();
          }
        }}
        // Typing to all, a right-click anywhere pastes to all; typing to one,
        // only that one's own right-click paste works.
        onContextMenuCapture={(e) => {
          if (!broadcast) return;
          if (solo === null) {
            e.preventDefault();
            e.stopPropagation();
            onPasteToAll();
          } else if (!isSolo) {
            e.preventDefault();
            e.stopPropagation();
          }
        }}
      >
        {/* The padding takes the terminal's own scheme background. */}
        <div ref={slotRef} className="absolute inset-0 p-1 select-text" style={{ background: entry?.term.options.theme?.background ?? "#09090b" }} />
        {!entry && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-center px-3 bg-[#09090b]">
            <span className="text-[10.5px] font-semibold uppercase tracking-wider text-zinc-400">
              {waiting ? "Waiting for you" :
                status === "connected" ? "Opening the terminal…" :
                status === "connecting" ? "Connecting…" :
                status === "failed" ? "Couldn't connect" : "Disconnected"}
            </span>
            <div className="flex items-center gap-2">
              {(status === "failed" || status === "disconnected") && !waiting && (
                <button onClick={onReconnect} className="h-6 px-2 rounded bg-primary/10 text-primary hover:bg-primary/20 text-[10px] font-bold flex items-center gap-1">
                  <RotateCw size={10} /> Reconnect
                </button>
              )}
              {(waiting || status === "failed") && (
                <button onClick={onOpen} className="h-6 px-2 rounded bg-white/5 text-zinc-200 hover:bg-white/10 text-[10px] font-bold flex items-center gap-1">
                  <ExternalLink size={10} /> {waiting ? "Answer" : "Details"}
                </button>
              )}
            </div>
          </div>
        )}
        {entry && !isLive && (
          <div className="absolute top-0 inset-x-0 flex items-center gap-2 px-2 py-1 bg-rose-950/80 border-b border-rose-500/30 text-[10px] text-rose-200">
            <span className="flex-1 truncate">
              {waiting ? "Waiting for you — not taking input" :
                status === "connecting" ? "Reconnecting — not taking input" : "Disconnected — not taking input"}
            </span>
            {waiting ? (
              <button onClick={onOpen} className="font-bold hover:text-white">Answer</button>
            ) : status !== "connecting" && (
              <button onClick={onReconnect} className="font-bold hover:text-white">Reconnect</button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

// Send: a set of commands typed into every terminal taking input, as if
// typed there — for the things a paste is awkward for, like putting the same
// key into sixty servers' authorized_keys.
const SendToAllModal = ({ targets, total, snippets, onSend, onClose }: {
  targets: () => number;
  total: number;
  snippets: Snippet[];
  onSend: (text: string, run: boolean) => void;
  onClose: () => void;
}) => {
  const [text, setText] = useState("");
  const [run, setRun] = useState(true);
  const n = targets();
  const lines = text ? lineCount(text) : 0;
  const ready = text.trim().length > 0 && n > 0;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Send to all"
        className="w-full max-w-xl flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <Send size={13} className="text-orange-300" /> Send to all
          </span>
          <button onClick={onClose} className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>
        <div className="p-4 flex flex-col gap-3">
          <p className="text-[11.5px] text-zinc-400 leading-relaxed">
            Typed into {n === total ? `all ${plural(n, "terminal")}` : `the ${plural(n, "terminal")} taking input`} on this wall, line by line,
            as if you typed it there.{total - n > 0 && ` ${total - n} paused, not connected or joining at the next line ${total - n === 1 ? "is" : "are"} left out.`}
          </p>
          {snippets.length > 0 && (
            <select
              value=""
              onChange={(e) => {
                const s = snippets.find((x) => String(x.id) === e.target.value);
                if (s) setText((t) => (t && !t.endsWith("\n") ? `${t}\n` : t) + s.content);
              }}
              className="h-8 px-2 bg-zinc-900/60 border border-white/10 rounded-lg text-[12px] text-zinc-200 outline-none focus:border-primary/50"
            >
              <option value="">Insert a command from the Library…</option>
              {snippets.map((s) => <option key={s.id} value={s.id}>{s.title}</option>)}
            </select>
          )}
          <textarea
            autoFocus
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && ready) {
                e.preventDefault();
                onSend(text, run);
              }
            }}
            spellCheck={false}
            rows={8}
            placeholder={"echo 'ssh-ed25519 AAAA… me@laptop' >> ~/.ssh/authorized_keys"}
            className="w-full p-3 bg-black/40 border border-white/10 rounded-lg font-mono text-[12px] text-zinc-100 outline-none focus:border-primary/50 resize-y custom-scrollbar select-text"
          />
          <label className="flex items-center gap-2 text-[11.5px] text-zinc-300 select-none">
            <input type="checkbox" className="accent-orange-400" checked={run} onChange={(e) => setRun(e.target.checked)} />
            Press Enter after the last line
          </label>
        </div>
        <div className="px-4 py-3 border-t border-white/5 flex items-center justify-end gap-2">
          <button onClick={onClose} className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider text-zinc-300 hover:bg-white/5">Cancel</button>
          <button
            disabled={!ready}
            onClick={() => onSend(text, run)}
            title="Ctrl+Enter"
            className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-orange-500/20 border border-orange-400/40 text-orange-200 hover:bg-orange-500/30 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Send {lines > 0 ? plural(lines, "line") : ""} to {plural(n, "server")}
          </button>
        </div>
      </div>
    </div>
  );
};

export default WallView;

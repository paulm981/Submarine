// Typing to one, and pausing, on Broadcast's Wall (#218).
//
// DesktopApp's Wall is the original project's; these are the fork's parts of
// it, each placed by one line there:
// - `BroadcastTileChips`, on a tile's header while its server is ticked:
//   "Only this one" (type to it alone) and pause. A paused tile says
//   "Paused" in words; Ctrl-click (⌘) or Shift-click on pause picks tiles to
//   pause or resume together. One that joined partway through a line says
//   "Next line" until that line ends.
// - `BroadcastBar`, on the Wall's bar: who's typed to, in words, with "Type
//   to all", "Pause all", "Resume all", and what to do with the picked ones.
//   It also follows the keyboard: clicking into another tile while typing to
//   one types to that one; Ctrl+Shift+P pauses or resumes the tile you're in.
//   Its "Send…" opens Send to all, for a set of commands (sendToAllDialog).
// Who gets the keys is broadcastAim's to say (src/fork/terminal/broadcastAim.ts).
import { useEffect, useRef, useState } from "react";
import { Pause, Play, Send, Target, X } from "lucide-react";
import { useBroadcast } from "../../ui/broadcast";
import {
  aimNow, clearPicks, pick, sendTargets, setOrder, setPaused, togglePaused, typeToAll, typeToOne, useAim,
} from "../terminal/broadcastAim";
import { broadcastSend } from "../terminal/broadcast";
import { SendToAllDialog } from "./sendToAllDialog";

// Each Broadcast tile's own element, for the keyboard: which tile it's in.
const tileRoots = new Map<string, HTMLElement>();

/** The tile a header's part is in: the header's parent, if the header is its
 *  first child and it holds no more than one terminal. Anything else (the
 *  Wall laid out differently after a sync) gets nothing, rather than the
 *  wrong terminal. */
const tileOf = (part: HTMLElement | null): HTMLElement | null => {
  const header = part?.parentElement;
  const tile = header?.parentElement;
  if (!header || !tile || tile.firstElementChild !== header) return null;
  return tile.querySelectorAll(".xterm-helper-textarea").length <= 1 ? tile : null;
};

/** Put the keyboard in a tile's terminal. */
const focusTile = (terminalId: string) => {
  const box = tileRoots.get(terminalId)?.querySelectorAll<HTMLElement>(".xterm-helper-textarea");
  if (box?.length === 1) box[0].focus();
};

/** The Broadcast tile the keyboard is in, if any. */
const tileWithKeys = (): string | null => {
  const active = document.activeElement;
  if (!(active instanceof Node)) return null;
  for (const [tid, root] of tileRoots) if (root.contains(active)) return tid;
  return null;
};

const isLive = (bc: ReturnType<typeof useBroadcast>) => bc.enabled && bc.targetSessionIds.size >= 2;

const chip = "h-5 px-1.5 rounded flex items-center gap-1 text-[9.5px] font-bold uppercase tracking-wider shrink-0 transition-all";

/** A Broadcast tile's buttons: type to it alone, and pause it. */
export const BroadcastTileChips = ({ sessionId, terminalId }: { sessionId: string; terminalId: string }) => {
  const bc = useBroadcast();
  const aim = useAim();
  const ref = useRef<HTMLSpanElement>(null);
  const on = isLive(bc) && bc.targetSessionIds.has(sessionId);
  useEffect(() => {
    if (!on) return;
    const tile = tileOf(ref.current);
    if (!tile) return;
    tileRoots.set(terminalId, tile);
    return () => {
      if (tileRoots.get(terminalId) === tile) tileRoots.delete(terminalId);
    };
  }, [on, terminalId]);
  if (!on) return null;
  const solo = aim.solo === terminalId;
  const paused = aim.paused.has(terminalId);
  const waiting = !paused && aim.line !== null && !aim.line.has(terminalId);
  const picked = aim.picked.includes(terminalId);
  const onPause = (e: React.MouseEvent) => {
    if (e.ctrlKey || e.metaKey || e.shiftKey) pick(terminalId, e.shiftKey);
    else togglePaused(terminalId);
  };
  return (
    <span ref={ref} className="flex items-center gap-1 shrink-0">
      {solo ? (
        <button
          onClick={typeToAll}
          title="Type to all of them again"
          className={`${chip} bg-primary/20 text-primary border border-primary/40`}
        >
          Only this one <X size={10} />
        </button>
      ) : (
        <button
          onClick={() => { typeToOne(terminalId); focusTile(terminalId); }}
          title="Type to this one only"
          aria-label="Type to this one only"
          className="h-5 w-5 rounded flex items-center justify-center text-zinc-500 hover:text-primary hover:bg-white/[0.05] transition-all"
        >
          <Target size={11} />
        </button>
      )}
      {waiting && (
        <span title="Joined partway through a line: what's typed reaches it from the next line" className={`${chip} text-zinc-300 border border-white/15`}>
          Next line
        </span>
      )}
      {picked && <span className={`${chip} text-sky-300 border border-sky-400/40`}>Picked</span>}
      <button
        onClick={onPause}
        aria-pressed={paused}
        title={paused
          ? "Resume: take what's typed again (Ctrl+Shift+P). Ctrl-click or Shift-click to pick several"
          : "Pause: leave this one out (Ctrl+Shift+P). Ctrl-click or Shift-click to pick several"}
        aria-label={paused ? "Resume" : "Pause"}
        className={paused
          ? `${chip} bg-amber-500/15 text-amber-300 border border-amber-400/40`
          : "h-5 w-5 rounded flex items-center justify-center text-zinc-500 hover:text-amber-300 hover:bg-white/[0.05] transition-all"}
      >
        {paused ? <><Play size={9} /> Paused</> : <Pause size={11} />}
      </button>
    </span>
  );
};

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

/** The Wall bar's Broadcast line: who's typed to, and the buttons for it. */
export const BroadcastBar = ({ tiles, sessions, statuses }: {
  tiles: { sessionId: string; terminalId: string }[];
  sessions: { id: string; serverName: string }[];
  /** Each session's state: only a connected one is sent to. */
  statuses: Record<string, string>;
}) => {
  const bc = useBroadcast();
  const aim = useAim();
  const [sendOpen, setSendOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const noteTimer = useRef(0);
  useEffect(() => () => window.clearTimeout(noteTimer.current), []);
  const flash = (msg: string) => {
    setNote(msg);
    window.clearTimeout(noteTimer.current);
    noteTimer.current = window.setTimeout(() => setNote(null), 4000);
  };
  const live = isLive(bc);
  const ours = live ? tiles.filter((t) => bc.targetSessionIds.has(t.sessionId)) : [];
  const order = ours.map((t) => t.terminalId);
  const orderKey = order.join("\n");

  useEffect(() => {
    setOrder(orderKey ? orderKey.split("\n") : []);
  }, [orderKey]);

  // Typing to one: clicking into another tile types to that one.
  const aimSolo = useRef(aim.solo);
  aimSolo.current = aim.solo;
  useEffect(() => {
    if (!live) return;
    const onFocus = () => {
      const tid = tileWithKeys();
      const now = aimSolo.current;
      if (tid && now !== null && now !== tid) typeToOne(tid);
    };
    document.addEventListener("focusin", onFocus, true);
    return () => document.removeEventListener("focusin", onFocus, true);
  }, [live]);

  // Ctrl+Shift+P: pause or resume the tile you're in. Taken before the
  // terminal sees it, only in a Broadcast tile; elsewhere the shell gets
  // Ctrl+P as ever.
  useEffect(() => {
    if (!live) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || !e.shiftKey || e.altKey || e.code !== "KeyP") return;
      const tid = tileWithKeys();
      if (!tid) return;
      e.preventDefault();
      e.stopPropagation();
      if (!e.repeat) togglePaused(tid);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [live]);

  // Broadcast ended with the box open: it goes, unsent.
  useEffect(() => {
    if (!live) setSendOpen(false);
  }, [live]);

  if (!live || !ours.length) return null;
  // Send to all: the connected ones, then who's taking what's typed now.
  const sendReach = () => {
    const now = ours.filter((t) => bc.targetSessionIds.has(t.sessionId));
    const connected = now.filter((t) => statuses[t.sessionId] === "connected").map((t) => t.terminalId);
    const r = sendTargets(aimNow(), connected);
    return { ids: r.to, to: r.to.length, total: now.length, paused: r.paused, waiting: r.waiting, offline: now.length - connected.length };
  };
  const pausedHere = order.filter((t) => aim.paused.has(t));
  const waiting = aim.line === null ? 0 : order.filter((t) => !aim.paused.has(t) && !aim.line?.has(t)).length;
  const reaching = ours.length - pausedHere.length - waiting;
  const leftOut = [pausedHere.length > 0 && `${pausedHere.length} paused`, waiting > 0 && `${waiting} at the next line`].filter(Boolean);
  const soloTile = aim.solo === null ? null : ours.find((t) => t.terminalId === aim.solo);
  const soloName = soloTile ? sessions.find((s) => s.id === soloTile.sessionId)?.serverName : null;
  const picked = aim.picked.filter((t) => order.includes(t));
  const btn = "px-2 py-0.5 rounded text-[9.5px] font-bold uppercase tracking-wider border shrink-0 transition-all";
  return (
    <div className="flex items-center gap-1.5 min-w-0 shrink overflow-hidden" role="group" aria-label="Broadcast">
      <span className="text-[10px] font-semibold text-zinc-300 truncate" aria-live="polite">
        {aim.solo !== null
          ? `Typing to ${soloName ?? "one terminal"} only`
          : leftOut.length > 0
            ? `Typing to ${reaching} of ${ours.length} · ${leftOut.join(" · ")}`
            : `Typing to all ${ours.length}`}
      </span>
      {aim.solo !== null && (
        <button onClick={typeToAll} title="Type to all of them again" className={`${btn} bg-primary/15 text-primary border-primary/40 hover:bg-primary/25`}>
          Type to all
        </button>
      )}
      {picked.length > 0 ? (
        <>
          <span className="text-[10px] text-sky-300 shrink-0">{plural(picked.length, "picked")}</span>
          <button onClick={() => setPaused(picked, true)} className={`${btn} text-zinc-300 border-white/10 hover:bg-white/[0.06]`}>Pause</button>
          <button onClick={() => setPaused(picked, false)} className={`${btn} text-zinc-300 border-white/10 hover:bg-white/[0.06]`}>Resume</button>
          <button onClick={clearPicks} title="Unpick them" aria-label="Unpick them" className="h-5 w-5 rounded flex items-center justify-center text-zinc-500 hover:text-zinc-200">
            <X size={10} />
          </button>
        </>
      ) : (
        <>
          {pausedHere.length < ours.length && (
            <button onClick={() => setPaused(order, true)} title="Leave all of them out until resumed" className={`${btn} text-zinc-400 border-white/10 hover:bg-white/[0.06]`}>
              Pause all
            </button>
          )}
          {pausedHere.length > 0 && (
            <button onClick={() => setPaused(pausedHere, false)} title="Every one takes what's typed again" className={`${btn} text-amber-300 border-amber-400/40 hover:bg-amber-500/15`}>
              Resume all
            </button>
          )}
        </>
      )}
      <button onClick={() => setSendOpen(true)} title="Type a set of commands into all of them" className={`${btn} text-orange-300 border-orange-400/40 hover:bg-orange-500/15 flex items-center gap-1`}>
        <Send size={10} /> Send…
      </button>
      {note && <span role="status" className="text-[10px] text-zinc-300 truncate">{note}</span>}
      {sendOpen && (
        <SendToAllDialog
          reach={sendReach}
          onClose={() => setSendOpen(false)}
          onSend={(data, lines, cleaned) => {
            const r = sendReach();
            setSendOpen(false);
            if (!r.ids.length) return flash("No server is taking what's typed right now.");
            broadcastSend(r.ids, data);
            flash(`Sent ${plural(lines, "line")} to ${plural(r.ids.length, "server")}.` + (cleaned ? " Control characters were left out." : ""));
          }}
        />
      )}
    </div>
  );
};

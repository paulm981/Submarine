// Terminals opened a few at a time (#127).
//
// Opening an xterm lays out the page more than once, and the page grows with
// every terminal already on it: a Wall of 36 tiles, or a profile's 36 servers
// starting together, opened every terminal in one go and held the window for
// seconds. TerminalTurn holds a terminal back until its turn: the first of a
// burst opens at once, the rest a few each frame, fewer while frames run
// slow, so the window answers in between. Only when it opens changes; what it
// does once open doesn't.
import { useEffect, useState, type ReactNode } from 'react';

/** Terminals let open in one frame: where it starts, and its bounds. */
const START = 2;
export const MOST_PER_FRAME = 8;
/** A frame longer than this halves the next frame's turns; one shorter than
 *  QUICK_FRAME_MS adds a turn. */
export const SLOW_FRAME_MS = 50;
export const QUICK_FRAME_MS = 20;

/** The next frame's turns, from this one's length. */
export const nextTurns = (turns: number, frameMs: number): number =>
  frameMs > SLOW_FRAME_MS ? Math.max(1, Math.floor(turns / 2)) : frameMs < QUICK_FRAME_MS ? Math.min(MOST_PER_FRAME, turns + 1) : turns;

const waiting: (() => void)[] = [];
let perFrame = START;
/** Turns given since the last frame, the first of a burst's among them. */
let givenThisFrame = 0;
let ticking = false;
let lastFrame = 0;

const tick = (now: number) => {
  if (lastFrame) perFrame = nextTurns(perFrame, now - lastFrame);
  lastFrame = now;
  givenThisFrame = 0;
  for (let i = 0; i < perFrame && waiting.length > 0; i++) {
    givenThisFrame++;
    waiting.shift()?.();
  }
  if (waiting.length > 0 || givenThisFrame > 0) {
    requestAnimationFrame(tick);
  } else {
    // Quiet again: the next terminal opens at once.
    ticking = false;
    lastFrame = 0;
    perFrame = START;
  }
};

const startTicking = () => {
  if (ticking) return;
  ticking = true;
  requestAnimationFrame(tick);
};

/** Whether a terminal may open now rather than wait: the first since the
 *  page was last quiet. Counts it as given. */
const openNow = (): boolean => {
  if (givenThisFrame > 0 || waiting.length > 0) return false;
  givenThisFrame = 1;
  startTicking();
  return true;
};

/** Wait for a turn; returns a function that gives it up. */
const awaitTurn = (go: () => void): (() => void) => {
  waiting.push(go);
  startTicking();
  return () => {
    const at = waiting.indexOf(go);
    if (at >= 0) waiting.splice(at, 1);
  };
};

/** Its children, once it's this terminal's turn to open. `now`: the one
 *  you're looking at, which never waits. */
export const TerminalTurn = ({ now = false, children }: { now?: boolean; children: ReactNode }) => {
  const [open, setOpen] = useState(() => now || openNow());
  useEffect(() => {
    if (open) return;
    if (now) {
      setOpen(true);
      return;
    }
    return awaitTurn(() => setOpen(true));
  }, [open, now]);
  return open ? <>{children}</> : null;
};

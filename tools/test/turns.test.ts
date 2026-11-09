// Terminals open a few at a time (#192), and still open in a hidden window
// (#205): the queue in turns.tsx, its frames and its timer driven by hand.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { awaitTurn, HIDDEN_TURN_MS, MOST_PER_FRAME, openNow } from "../../src/fork/terminal/turns";

let frames: ((now: number) => void)[] = [];
let now = 1000;
const page = { hidden: false };

/** One frame of `ms`. */
const frame = (ms = 10) => {
  now += ms;
  for (const f of frames.splice(0)) f(now);
};

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("requestAnimationFrame", (f: (now: number) => void) => frames.push(f));
  vi.stubGlobal("document", page);
  page.hidden = false;
});

afterEach(() => {
  // Until it's quiet again, so the next test starts from a quiet page.
  page.hidden = false;
  for (let i = 0; i < 100 && frames.length; i++) frame();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** `n` terminals waiting; what's opened, by number, in order. */
const burst = (n: number) => {
  const opened: number[] = [];
  const cancels = Array.from({ length: n }, (_, i) => awaitTurn(() => opened.push(i)));
  return { opened, cancels };
};

describe("terminals open a few at a time (#192)", () => {
  it("opens the first at once, then a few each frame, more while frames are quick", () => {
    expect(openNow()).toBe(true);
    // A burst is on: the next one waits.
    expect(openNow()).toBe(false);
    const { opened } = burst(9);
    expect(opened).toEqual([]);
    frame();
    expect(opened).toEqual([0, 1]);
    frame();
    expect(opened).toEqual([0, 1, 2, 3, 4]);
    frame();
    expect(opened.length).toBe(9);
  });

  it("never opens one that went away before its turn, nor gives it a turn", () => {
    expect(openNow()).toBe(true);
    const { opened, cancels } = burst(3);
    cancels[0]();
    frame();
    expect(opened).toEqual([1, 2]);
  });

  it("still opens one a frame while frames run slow", () => {
    expect(openNow()).toBe(true);
    const { opened } = burst(6);
    // The first frame can't tell how long frames are: two, as always.
    frame(200);
    expect(opened.length).toBe(2);
    for (let i = 3; i <= 6; i++) {
      frame(200);
      expect(opened.length).toBe(i);
    }
  });

  it("starts afresh once quiet: the next burst's first frame opens two again", () => {
    expect(openNow()).toBe(true);
    const { opened } = burst(6);
    frame(200);
    frame(200);
    for (let i = 0; i < 10 && frames.length; i++) frame();
    expect(opened.length).toBe(6);
    expect(openNow()).toBe(true);
    const next = burst(5);
    frame();
    expect(next.opened).toEqual([0, 1]);
  });
});

describe("in a hidden window, which gets no frames (#205)", () => {
  it("opens them by a timer, a frame's worth at a time", () => {
    page.hidden = true;
    expect(openNow()).toBe(true);
    const { opened } = burst(20);
    // No frame ever comes.
    vi.advanceTimersByTime(HIDDEN_TURN_MS);
    expect(opened.length).toBe(MOST_PER_FRAME);
    vi.advanceTimersByTime(2 * HIDDEN_TURN_MS);
    expect(opened.length).toBe(20);
  });

  it("takes a turn from the frame or the timer, never both", () => {
    page.hidden = true;
    expect(openNow()).toBe(true);
    const { opened } = burst(30);
    vi.advanceTimersByTime(HIDDEN_TURN_MS);
    expect(opened.length).toBe(MOST_PER_FRAME);
    // The frame asked for with that timer comes late, with the next one:
    // only the next one gives turns.
    expect(frames.length).toBe(2);
    frame();
    expect(opened.length).toBe(2 * MOST_PER_FRAME);
    // And the timer that frame came with gives none.
    vi.advanceTimersByTime(HIDDEN_TURN_MS - 1);
    expect(opened.length).toBe(2 * MOST_PER_FRAME);
  });
});

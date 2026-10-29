// Which terminals draw with the GPU (xterm's WebGL renderer). A webview keeps
// only about 16 WebGL contexts; past that it takes the oldest away, so in a
// Wall of 36 the ones that kept theirs were simply the last to open, hidden
// tabs among them, while terminals on screen drew without the GPU (#127).
// Here at most MAX_GPU terminals hold one, and a terminal on screen gets one
// first: from the free ones, else from a terminal that isn't on screen.
// A hidden terminal keeps its context until one on screen needs it, so going
// back and forth between a few tabs doesn't rebuild anything.

/** Fewer than the webview's ~16, so that its own and other pages' contexts
 *  never push a terminal's out. */
export const MAX_GPU = 12;

export interface GpuHolder {
  /** Whether the terminal is on screen now (its tab in front, a pane of the
   *  split, or a Wall's tile). */
  onScreen: () => boolean;
  /** Give the GPU up: dispose the WebGL addon and draw with the DOM. */
  evict: () => void;
  /** A context may be free now: try for one again. */
  retry: () => void;
}

const holding = new Set<GpuHolder>();
const waiting = new Set<GpuHolder>();

/** Asks for a context for `h`, which is on screen. True when it has one;
 *  else it waits, and `retry` is called when one may be free. */
export function claimGpu(h: GpuHolder): boolean {
  if (holding.has(h)) return true;
  if (holding.size >= MAX_GPU) {
    const idle = [...holding].find((o) => !o.onScreen());
    if (!idle) {
      waiting.add(h);
      return false;
    }
    holding.delete(idle);
    idle.evict();
  }
  waiting.delete(h);
  holding.add(h);
  return true;
}

/** `h` no longer holds or wants a context (closed, GPU drawing off, or its
 *  context was lost). */
export function releaseGpu(h: GpuHolder): void {
  waiting.delete(h);
  if (holding.delete(h)) gpuChanged();
}

/** A context became free, or a holder went off screen: terminals on screen
 *  that wait for one try again. */
export function gpuChanged(): void {
  for (const w of [...waiting]) {
    if (!w.onScreen()) continue;
    waiting.delete(w);
    w.retry();
  }
}

/** For the diagnostics: how many hold a context, and how many wait. */
export const gpuSlotsInUse = (): { holding: number; waiting: number } => ({ holding: holding.size, waiting: waiting.size });

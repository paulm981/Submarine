// The DOM renderer's character widths, shared by every terminal (#127).
//
// xterm's DOM renderer, which draws each terminal without a GPU context
// (src/ui/gpuSlots.ts), measures the width of each character the first time
// it draws it: it puts the character in a hidden element and reads its width,
// and each read makes the browser lay out the page again. Every terminal has a
// cache of its own, so 36 terminals opening at once (a Wall, or a profile's
// servers starting) measured the same few dozen characters 36 times over, and
// each measurement laid out a page that 36 new terminals had just changed: the
// window held for seconds.
//
// Here the measurements go into one cache for the whole page: a character
// measured once, in one font, size, weight and style, isn't measured again by
// another terminal. xterm's own cache still sits in front of it.
//
// It reaches into xterm's private WidthCache, as TerminalView's selection
// fix reaches into its render service: found through a terminal's renderer,
// and left alone when it isn't there, so a newer xterm only loses the
// sharing, never a terminal.
import type { Terminal } from '@xterm/xterm';

/** Widths kept at most; past it the cache starts again. */
export const MAX_WIDTHS = 20000;

const widths = new Map<string, number>();
let shared = false;

/** What a width depends on, besides the text: the measuring cache's font,
 *  size, and weight for `variant` (1: bold, 2: italic, as xterm numbers its
 *  measuring elements), and the screen's pixel ratio. */
interface Measurer {
  _font: string;
  _fontSize: number;
  _weight: string | number;
  _weightBold: string | number;
}

export const widthKey = (m: Measurer, text: string, variant: number, ratio: number): string =>
  [m._font, m._fontSize, variant & 1 ? m._weightBold : m._weight, variant & 2 ? 'i' : '', ratio, text].join('\u0000');

/** Wrap `measure` (a WidthCache's `_measure`) so a width is measured once
 *  for every terminal. A width of 0, from a terminal that isn't shown, is
 *  never kept, as xterm keeps none either. */
export const sharedMeasure = (measure: (this: Measurer, text: string, variant: number) => number) =>
  function (this: Measurer, text: string, variant: number): number {
    const key = widthKey(this, text, variant, window.devicePixelRatio);
    const known = widths.get(key);
    if (known !== undefined) return known;
    const width = measure.call(this, text, variant);
    if (width > 0) {
      if (widths.size >= MAX_WIDTHS) widths.clear();
      widths.set(key, width);
    }
    return width;
  };

/** Share the widths of every terminal's DOM renderer, from the first
 *  terminal opened (`term.open` done) whose renderer is a DOM one. */
export const shareWidths = (term: Terminal): void => {
  if (shared) return;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const cache = (term as any)._core?._renderService?._renderer?.value?._widthCache;
  const proto = cache ? Object.getPrototypeOf(cache) : null;
  if (!proto || typeof proto._measure !== 'function' || !('_font' in cache) || !('_weightBold' in cache)) return;
  proto._measure = sharedMeasure(proto._measure);
  shared = true;
  // A font that finished loading draws at other widths than the fallback
  // measured before it.
  document.fonts?.addEventListener?.('loadingdone', () => widths.clear());
};

/** For the tests: forget every width. */
export const clearWidths = () => widths.clear();

import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Dragging a tab to a new place (server tabs in the title bar, terminal tabs
// in a server). Pointer events, not HTML5 drag-and-drop: the desktop webview
// can take file-drop for itself and never deliver the drag events (the server
// grid's and the session tree's drags do the same for this reason).
//
// A tab carries `data-tab-group` (which strip it is in) and `data-tab` (its
// id); anything inside it marked `data-no-tab-drag` (its close button)
// doesn't start a drag. The order lasts as long as the tabs are open: the
// caller reorders its own array in `move`.
//
// As in the session tree: Escape, leaving the window (Alt+Tab) or letting go
// of the button outside it ends the drag where it started; holding the
// pointer near either end of a strip that scrolls scrolls it.
// ─────────────────────────────────────────────────────────────────────────────

/// `list` with the item `dragId` moved next to `overId` (after it, or before).
export const reorderById = <T extends { id: string }>(list: T[], dragId: string, overId: string, after: boolean): T[] => {
  if (dragId === overId) return list;
  const item = list.find(x => x.id === dragId);
  if (!item || !list.some(x => x.id === overId)) return list;
  const rest = list.filter(x => x.id !== dragId);
  const at = rest.findIndex(x => x.id === overId);
  rest.splice(after ? at + 1 : at, 0, item);
  return rest;
};

// Moving less than this is still a click. A little less than the session
// tree's 8: a tab only moves sideways and is short, so a small move should
// already show where it would go, and a click hardly ever slides 6px.
const DRAG_START_PX = 6;
// This close to either end of the strip, the strip scrolls.
const EDGE_PX = 28;
const SCROLL_STEP_PX = 12;

interface TabDrag { id: string; over: string | null; after: boolean }

// The nearest ancestor that scrolls sideways: the tab strip.
const scrollingStrip = (el: HTMLElement): HTMLElement | null => {
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(p).overflowX) && p.scrollWidth > p.clientWidth) return p;
  }
  return null;
};

export const useTabDrag = (group: string, move: (dragId: string, overId: string, after: boolean) => void) => {
  const [drag, setDrag] = useState<TabDrag | null>(null);
  const moveRef = useRef(move);
  moveRef.current = move;
  // Ends the drag in progress (removes its window listeners), if any.
  const stopRef = useRef<(() => void) | null>(null);
  useEffect(() => () => stopRef.current?.(), []);

  const begin = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("[data-no-tab-drag]")) return;
    stopRef.current?.();
    const tab = e.currentTarget as HTMLElement;
    const sx = e.clientX, sy = e.clientY;
    let px = sx, py = sy;
    let started = false;
    let strip: HTMLElement | null = null;
    let timer = 0;
    let shown: TabDrag | null = null;
    // Re-render only when the mark changes, not on every pointer move: the
    // title bar's strip is all of DesktopApp.
    const show = (next: TabDrag | null) => {
      if (shown === next || (shown && next && shown.over === next.over && shown.after === next.after)) return;
      shown = next;
      setDrag(next);
    };
    const locate = () => {
      const el = document.elementFromPoint(px, py)?.closest<HTMLElement>(`[data-tab-group="${group}"]`);
      if (el && el.dataset.tab && el.dataset.tab !== id) {
        const r = el.getBoundingClientRect();
        show({ id, over: el.dataset.tab, after: px > r.left + r.width / 2 });
      } else {
        show({ id, over: null, after: false });
      }
    };
    const autoScroll = () => {
      if (!strip) return;
      const r = strip.getBoundingClientRect();
      if (py < r.top || py > r.bottom) return;
      if (px < r.left + EDGE_PX) strip.scrollLeft -= SCROLL_STEP_PX;
      else if (px > r.right - EDGE_PX) strip.scrollLeft += SCROLL_STEP_PX;
      else return;
      // The tabs moved under a still pointer.
      locate();
    };
    const onMove = (ev: PointerEvent) => {
      // The button came up somewhere this never heard of (outside the
      // window, say): nothing is being held any more.
      if ((ev.buttons & 1) === 0) { cancel(); return; }
      px = ev.clientX; py = ev.clientY;
      if (!started) {
        if (Math.hypot(px - sx, py - sy) < DRAG_START_PX) return;
        started = true;
        strip = scrollingStrip(tab);
        timer = window.setInterval(autoScroll, 30);
      }
      locate();
    };
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape" && started) { ev.preventDefault(); cancel(); }
    };
    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
      window.removeEventListener("keydown", onKey);
      window.clearInterval(timer);
      if (stopRef.current === stop) stopRef.current = null;
    };
    // Dropped nowhere: everything stays where it was.
    function cancel() {
      stop();
      if (started) show(null);
    }
    function end(ev: PointerEvent) {
      stop();
      if (!started) return;
      // Where the pointer is now, not where the last move left it.
      px = ev.clientX; py = ev.clientY;
      locate();
      const at = shown;
      show(null);
      if (at?.over) moveRef.current(id, at.over, at.after);
      // The click that follows letting go would also select the tab.
      const swallow = (c: Event) => { c.stopPropagation(); c.preventDefault(); };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    window.addEventListener("keydown", onKey);
    stopRef.current = stop;
  };

  /// Props for a tab: what starts the drag and tells the hook where it is.
  const tabProps = (id: string) => ({
    "data-tab-group": group,
    "data-tab": id,
    onPointerDown: (e: ReactPointerEvent) => begin(e, id),
  });
  /// While dragging: the tab being dragged is dimmed, and the one it is over
  /// gets a line on the side it would land.
  const tabMark = (id: string): { dim: boolean; style?: { boxShadow: string } } => {
    if (!drag) return { dim: false };
    if (drag.id === id) return { dim: true };
    if (drag.over === id) {
      return { dim: false, style: { boxShadow: `inset ${drag.after ? "-" : ""}2px 0 0 0 var(--primary-hex)` } };
    }
    return { dim: false };
  };
  return { tabProps, tabMark };
};

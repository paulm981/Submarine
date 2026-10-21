import { useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Dragging a tab to a new place (server tabs in the title bar, terminal tabs
// in a server). Pointer events, not HTML5 drag-and-drop: the desktop webview
// can take file-drop for itself and never deliver the drag events (the server
// grid's drag does the same for this reason).
//
// A tab carries `data-tab-group` (which strip it is in) and `data-tab-id`;
// anything inside it marked `data-no-tab-drag` (its close button) doesn't
// start a drag. The order lasts as long as the tabs are open: the caller
// reorders its own array in `move`.
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

// Moving less than this is still a click.
const DRAG_START_PX = 6;

interface TabDrag { id: string; over: string | null; after: boolean }

export const useTabDrag = (group: string, move: (dragId: string, overId: string, after: boolean) => void) => {
  const [drag, setDrag] = useState<TabDrag | null>(null);
  const moveRef = useRef(move);
  moveRef.current = move;

  const begin = (e: ReactPointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("[data-no-tab-drag]")) return;
    const sx = e.clientX, sy = e.clientY;
    let started = false;
    let over: string | null = null;
    let after = false;
    const onMove = (ev: PointerEvent) => {
      if (!started) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < DRAG_START_PX) return;
        started = true;
      }
      const el = document.elementFromPoint(ev.clientX, ev.clientY)
        ?.closest<HTMLElement>(`[data-tab-group="${group}"]`);
      if (el && el.dataset.tabId && el.dataset.tabId !== id) {
        const r = el.getBoundingClientRect();
        over = el.dataset.tabId;
        after = ev.clientX > r.left + r.width / 2;
      } else {
        over = null;
      }
      setDrag({ id, over, after });
    };
    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", cancel);
    };
    const cancel = () => { stop(); setDrag(null); };
    const end = () => {
      stop();
      if (!started) return;
      setDrag(null);
      if (over) moveRef.current(id, over, after);
      // The click that follows letting go would also select the tab.
      const swallow = (ev: Event) => { ev.stopPropagation(); ev.preventDefault(); };
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", cancel);
  };

  /// Props for a tab: what starts the drag and tells the hook where it is.
  const tabProps = (id: string) => ({
    "data-tab-group": group,
    "data-tab-id": id,
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

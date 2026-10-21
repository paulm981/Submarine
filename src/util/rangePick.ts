// Shift-click picking: the items from the anchor (the last one picked
// without Shift) to `to`, in `order`, on top of `base` — what was picked
// before the run of Shift-clicks began. Each Shift-click from the same
// anchor replaces the run, so it grows and shrinks; it never drops what was
// picked before it. Null when the anchor or `to` isn't in `order` (nothing
// to range from).
export const rangePick = <K>(order: readonly K[], anchor: K, to: K, base: Iterable<K>): K[] | null => {
  const a = order.indexOf(anchor);
  const b = order.indexOf(to);
  if (a < 0 || b < 0) return null;
  return [...new Set([...base, ...order.slice(Math.min(a, b), Math.max(a, b) + 1)])];
};

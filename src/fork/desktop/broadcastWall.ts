// Broadcast on one screen (#218): every ticked server's terminal on the
// Wall, and typing into any of them goes to all of them.
//
// - **Ticking is turning it on.** Ticking servers in the Broadcast menu turns
//   Broadcast on; unticking the last one turns it off. The menu's own
//   switch ticks every open session, or none (`broadcastSwitch`). There's no
//   longer a set of ticked servers with Broadcast itself off.
// - **Its own Wall.** Once two or more are ticked, the Wall opens with each
//   ticked server's terminal on it (the one in front in its tab). Ticking
//   another adds its tile, unticking takes it off. Terminals pinned by hand
//   stay as they are: only the tiles Broadcast put there come and go
//   (`syncBroadcastTiles`). When it ends, the window goes back to where it
//   was if the Wall is left empty.
// - The original project's Broadcast and Wall, as they are: this only ticks,
//   pins and unpins through them (src/ui/broadcast.tsx, DesktopApp's
//   `wallItems`).
// - **When it ends**, who's typed to and what's paused is forgotten
//   (broadcastAim), so next time every ticked server gets the keys.
// - **One tab** (#224): while it's on, the ticked servers' tabs fold into
//   one Broadcast tab, the Wall (`shownTabs`, BroadcastTab). Going to a
//   ticked server's own view (its tile's "open in its own tab", the all-tabs
//   list, the session tree) takes it out of Broadcast, and its tab comes
//   back: that's popping it out. A server ticked while its tab is in front
//   folds away, and the window goes to the Wall.
import { useEffect, useRef, type Dispatch, type SetStateAction } from "react";
import { resetAim } from "../terminal/broadcastAim";
import { serverProtocol } from "../../util/serverProtocol";

export type WallTile = { id: string; sessionId: string; terminalId: string };

/** Broadcast's servers, `targets` (each one's terminal in front, if it has
 *  one yet), as the Wall's tiles: `tiles` with those Broadcast put there and
 *  that aren't targets any more taken off, and each target's terminal added
 *  if it isn't there, Broadcast's tiles in the order the servers are ticked.
 *  `ours`: the ids of the tiles Broadcast put there. Returns the tiles and
 *  the new `ours`; the same `tiles` when nothing changes. */
export const syncBroadcastTiles = (
  tiles: WallTile[],
  targets: { sessionId: string; terminalId?: string }[],
  ours: ReadonlySet<string>,
): { tiles: WallTile[]; ours: Set<string> } => {
  const want = new Map<string, WallTile>();
  for (const t of targets) {
    if (t.terminalId) want.set(`wall-${t.terminalId}`, { id: `wall-${t.terminalId}`, sessionId: t.sessionId, terminalId: t.terminalId });
  }
  const kept = tiles.filter((t) => !ours.has(t.id) || want.has(t.id));
  const have = new Set(kept.map((t) => t.id));
  const added = [...want.values()].filter((t) => !have.has(t.id));
  const next = added.length || kept.length !== tiles.length ? [...kept, ...added] : tiles;
  // Ours: what we added, and what we had that's still wanted. A tile pinned
  // by hand before isn't ours, and stays when Broadcast ends.
  const nextOurs = new Set([...[...ours].filter((id) => want.has(id)), ...added.map((t) => t.id)]);
  return { tiles: next, ours: nextOurs };
};

/** The servers of `list` Broadcast can type to: those with a terminal (SSH),
 *  not RDP, VNC, Telnet or FTP. For Broadcast to a folder or a pick (#218). */
export const broadcastable = <S extends { protocol?: string | null }>(list: S[]): S[] =>
  list.filter((s) => serverProtocol(s.protocol) === "ssh");

type Broadcast = {
  enabled: boolean;
  targetSessionIds: Set<string>;
  sessionTerminalMap: Record<string, string>;
  enable: () => void;
  disable: () => void;
  toggleTarget: (sessionId: string) => void;
  selectAll: (ids: string[]) => void;
  selectNone: () => void;
};

/** Broadcast is on, typing to two or more: the Wall is its screen. */
export const broadcastLive = (b: { enabled: boolean; targetSessionIds: ReadonlySet<string> }): boolean =>
  b.enabled && b.targetSessionIds.size >= 2;

/** The tabs the strip shows: while Broadcast is on, all but its servers',
 *  which fold into the Broadcast tab. The same `sessions` otherwise. */
export const shownTabs = <S extends { id: string }>(sessions: S[], b: { enabled: boolean; targetSessionIds: ReadonlySet<string> }): S[] =>
  broadcastLive(b) ? sessions.filter((s) => !b.targetSessionIds.has(s.id)) : sessions;

/** What showing `view` does while Broadcast is on, `wasLive` before this
 *  render too: going to one of its servers (`moved`, a change of view) pops
 *  it out; one of its servers in front without a move (just ticked) folds
 *  away to the Wall. */
export const onTickedView = (view: string, moved: boolean, wasLive: boolean, b: { enabled: boolean; targetSessionIds: ReadonlySet<string> }): "popOut" | "toWall" | null => {
  if (!broadcastLive(b) || !b.targetSessionIds.has(view)) return null;
  return moved && wasLive ? "popOut" : "toWall";
};

/** The Broadcast menu's switch: on ticks every open session, off unticks
 *  them all. */
export const broadcastSwitch = (broadcast: Pick<Broadcast, "enabled" | "targetSessionIds" | "selectAll" | "selectNone">, allSessionIds: string[]) => {
  if (broadcast.enabled || broadcast.targetSessionIds.size > 0) broadcast.selectNone();
  else broadcast.selectAll(allSessionIds);
};

/** DesktopApp's hook: Broadcast on while servers are ticked, and its servers
 *  on the Wall while two or more are. */
export const useBroadcastWall = ({ broadcast, frontTerminals, wallItems, setWallItems, activeView, setActiveView }: {
  broadcast: Broadcast;
  /** Each session's terminal in front, as its SessionView reports it
   *  (DesktopApp's `sessionActiveTerm`): known for a tab never looked at
   *  yet too, where Broadcast's own map isn't. */
  frontTerminals: Record<string, string>;
  wallItems: WallTile[];
  setWallItems: Dispatch<SetStateAction<WallTile[]>>;
  activeView: string;
  setActiveView: (view: string) => void;
}) => {
  const ours = useRef<Set<string>>(new Set());
  // Where the window was before Broadcast opened the Wall.
  const before = useRef<string | null>(null);
  const viewRef = useRef(activeView);
  viewRef.current = activeView;
  const { enabled, targetSessionIds, sessionTerminalMap, enable, disable, toggleTarget } = broadcast;
  const ticked = targetSessionIds.size;

  // Ticking is turning it on.
  useEffect(() => {
    if (ticked > 0 && !enabled) enable();
    if (ticked === 0 && enabled) disable();
  }, [ticked, enabled, enable, disable]);
  useEffect(() => {
    if (!enabled) resetAim();
  }, [enabled]);

  const live = enabled && ticked >= 2;
  useEffect(() => {
    const targets = live
      ? [...targetSessionIds].map((sessionId) => ({ sessionId, terminalId: frontTerminals[sessionId] ?? sessionTerminalMap[sessionId] }))
      : [];
    const r = syncBroadcastTiles(wallItems, targets, ours.current);
    ours.current = r.ours;
    if (r.tiles !== wallItems) setWallItems(r.tiles);
    if (live && before.current === null) {
      before.current = viewRef.current === "wall" ? "" : viewRef.current;
      if (viewRef.current !== "wall") setActiveView("wall");
    } else if (!live && before.current !== null) {
      const back = before.current;
      before.current = null;
      if (r.tiles.length === 0 && back && viewRef.current === "wall") setActiveView(back);
    }
    // A tile of Broadcast's unpinned by hand comes back while its server is
    // ticked: untick it to take it off.
  }, [live, targetSessionIds, frontTerminals, sessionTerminalMap, wallItems, setWallItems, setActiveView]);

  // One tab: popping a server out, or folding one ticked under you away.
  const wasLive = useRef(false);
  const lastView = useRef(activeView);
  useEffect(() => {
    const moved = lastView.current !== activeView;
    lastView.current = activeView;
    const what = onTickedView(activeView, moved, wasLive.current, broadcast);
    wasLive.current = live;
    if (what === "popOut") toggleTarget(activeView);
    else if (what === "toWall") setActiveView("wall");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeView, live, targetSessionIds, toggleTarget, setActiveView]);
};

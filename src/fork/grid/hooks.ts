import { useMemo, useState } from "react";
import type { MouseEvent } from "react";
import { type FolderRow, childFolders, folderPath, indexFolders, subtreeCounts, subtreeIds } from "../../util/folderPath";
import type { GridServer } from "./rules";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state in the server grid, as hooks NodeGrid calls in one line
// each (#163). Each keeps what NodeGrid kept, called where NodeGrid had it.
// ─────────────────────────────────────────────────────────────────────────────

/** The fork's nested folders (util/folderPath), in the shape of the original
 *  project's `folderTree` (util/folderTree), which NodeGrid reads as `tree`.
 *  The fork's differ from it in two ways the grid shows: a folder's folders
 *  come sorted by name, not in stored order (childrenOf), and a folder card
 *  counts every server inside it, at any depth, so a folder of folders isn't
 *  "0" (serverCount). The index and the counts are built once per change, not
 *  per card: an import brings hundreds. A folder whose parent is gone, or
 *  whose parents loop back to it, is at the top level, as there; so is a
 *  server whose folder is gone. */
export const useFolderTree = (folders: FolderRow[] | null | undefined, servers: GridServer[] | null | undefined) => {
  const allFolders: FolderRow[] = folders || [];
  const folderIndex = useMemo(() => indexFolders(allFolders), [folders]);
  const serverCounts = useMemo(() => subtreeCounts(folderIndex, (servers || []).map((s) => s.folder_id)), [folderIndex, servers]);
  return {
    has: (id: number) => folderIndex.byId.has(id),
    /** The folder a folder shows in (null: top level). */
    parentOf: (id: number) => folderIndex.parent.get(id) ?? null,
    /** The folders shown in `id` (null: top level), sorted by name. */
    childrenOf: (id: number | null) => childFolders(folderIndex, id),
    /** The folders from the top level down to `id`, `id` included. */
    pathOf: (id: number | null) => folderPath(folderIndex, id),
    /** `id` and every folder under it. */
    subtreeOf: (id: number) => new Set(subtreeIds(folderIndex, id)),
    /** The folder a server shows in (null: top level). */
    serverFolder: (s: GridServer) => (s.folder_id != null && folderIndex.byId.has(s.folder_id) ? s.folder_id : null),
    /** The servers inside a folder, at any depth. */
    serverCount: (id: number) => serverCounts.get(id) ?? 0,
  };
};

/** What a server card's menu reads of its server: the server itself goes to
 *  NodeGrid's onOpenServer and onEditServer as it is. */
export type MenuServer = { id: number; name?: string; host?: string; protocol?: string | null };
export type CardMenuAt = { x: number; y: number; server: MenuServer };

/** A server card's right-click menu (CardMenu): where it opened, and for
 *  which server. Not while a drag is ending (`dragged`). */
export const useCardMenu = () => {
  const [cardMenu, setCardMenu] = useState<CardMenuAt | null>(null);
  return {
    cardMenu,
    close: () => setCardMenu(null),
    open: (e: MouseEvent, server: MenuServer, dragged: boolean) => {
      e.preventDefault();
      if (dragged) return;
      setCardMenu({ x: e.clientX, y: e.clientY, server });
    },
  };
};

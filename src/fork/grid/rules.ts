import { jumpHostNote } from "../../util/jumpHostNote";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's rules for the server grid (NodeGrid), as plain functions NodeGrid
// calls in one line each (#163): NodeGrid.tsx is the original project's file,
// so every line the fork adds there can conflict at the next sync.
// ─────────────────────────────────────────────────────────────────────────────

/** What the fork reads of a server in the grid (DesktopApp's servers). */
export type GridServer = { id: number; name?: string; folder_id?: number | null; jump_host_id?: number | null };

/** The question before a folder is deleted, `inside` being it and every
 *  folder in it (useFolderTree's subtreeOf): all of it goes, and servers
 *  elsewhere that go through one inside it as their jump host are named
 *  (util/jumpHostNote). */
export const deleteFolderMessage = (f: { name?: string }, inside: Set<number>, servers: GridServer[] | null | undefined) => {
  const gone = new Set<number>((servers || []).filter((x) => x.folder_id != null && inside.has(x.folder_id)).map((x) => x.id));
  const note = jumpHostNote(servers, gone);
  return `“${f.name}” and every folder and server inside it will be removed.${note}`;
};

// Folders nest through `parent_id` (an import from MobaXterm brings whole
// trees). A folder whose parent is gone, or whose parents loop back to it
// (the backend's folder_subtree guards against sync producing one), counts
// as top level, so nothing is ever unreachable.

export type FolderRow = { id: number; name: string; parent_id?: number | null };

export type FolderIndex = {
  byId: Map<number, FolderRow>;
  // The parent each folder is shown under; null = top level.
  parent: Map<number, number | null>;
  // Folders shown under each parent, sorted by name.
  children: Map<number | null, FolderRow[]>;
};

const byName = (a: FolderRow, b: FolderRow) =>
  a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });

// Built once per change of the folder list; everything below reads it.
export const indexFolders = (folders: FolderRow[]): FolderIndex => {
  const byId = new Map(folders.map((f) => [f.id, f]));
  const parent = new Map<number, number | null>();
  for (const f of folders) {
    let p = f.parent_id ?? null;
    if (p != null && !byId.has(p)) p = null;
    // f is in a loop when walking up from it comes back to it. A loop
    // further up is left to its own members, which each find it.
    const seen = new Set<number>();
    for (let cur = p; cur != null; cur = byId.get(cur)?.parent_id ?? null) {
      if (cur === f.id) { p = null; break; }
      if (seen.has(cur)) break;
      seen.add(cur);
    }
    parent.set(f.id, p);
  }
  const children = new Map<number | null, FolderRow[]>();
  for (const f of folders) {
    const p = parent.get(f.id) ?? null;
    if (!children.has(p)) children.set(p, []);
    children.get(p)!.push(f);
  }
  for (const list of children.values()) list.sort(byName);
  return { byId, parent, children };
};

// Outermost first.
export const folderPath = (index: FolderIndex, id: number | null): FolderRow[] => {
  const out: FolderRow[] = [];
  for (let cur = id; cur != null && index.byId.has(cur); cur = index.parent.get(cur) ?? null) {
    out.unshift(index.byId.get(cur)!);
  }
  return out;
};

export const childFolders = (index: FolderIndex, parent: number | null): FolderRow[] =>
  index.children.get(parent) ?? [];

// How many items sit in each folder, counting everything in the folders
// inside it. `folderOf` lists each item's folder (unknown ones are ignored).
export const subtreeCounts = (index: FolderIndex, folderOf: (number | null | undefined)[]): Map<number, number> => {
  const counts = new Map<number, number>();
  for (const f of folderOf) {
    for (let cur = f ?? null; cur != null && index.byId.has(cur); cur = index.parent.get(cur) ?? null) {
      counts.set(cur, (counts.get(cur) ?? 0) + 1);
    }
  }
  return counts;
};

// For pickers: every folder as "Outer › Inner", sorted, so two folders that
// share a name in different places can be told apart.
export const folderOptions = (folders: FolderRow[]): { id: number; label: string }[] => {
  const index = indexFolders(folders);
  return folders
    .map((f) => ({ id: f.id, label: folderPath(index, f.id).map((x) => x.name).join(" › ") }))
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { numeric: true, sensitivity: "base" }));
};

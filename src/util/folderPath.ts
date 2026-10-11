// Folders nest through `parent_id` (an import from MobaXterm brings whole
// trees). A folder whose parent is gone counts as top level, so nothing is
// ever unreachable.

export type FolderRow = { id: number; name: string; parent_id?: number | null };

export const parentOf = (folders: FolderRow[], f: FolderRow): number | null => {
  const p = f.parent_id ?? null;
  return p != null && folders.some((x) => x.id === p) ? p : null;
};

// Outermost first. Stops at a loop rather than spinning.
export const folderPath = (folders: FolderRow[], id: number | null): FolderRow[] => {
  const out: FolderRow[] = [];
  const seen = new Set<number>();
  let cur = id == null ? undefined : folders.find((f) => f.id === id);
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    out.unshift(cur);
    const p = parentOf(folders, cur);
    cur = p == null ? undefined : folders.find((f) => f.id === p);
  }
  return out;
};

export const childFolders = (folders: FolderRow[], parent: number | null): FolderRow[] =>
  folders
    .filter((f) => f.id !== parent && parentOf(folders, f) === parent)
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }));

// The folder and every folder inside it.
export const subtreeIds = (folders: FolderRow[], id: number): Set<number> => {
  const out = new Set<number>([id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const f of folders) {
      const p = parentOf(folders, f);
      if (p != null && out.has(p) && !out.has(f.id)) { out.add(f.id); grew = true; }
    }
  }
  return out;
};

// For pickers: every folder as "Outer › Inner", sorted, so two folders that
// share a name in different places can be told apart.
export const folderOptions = (folders: FolderRow[]): { id: number; label: string }[] =>
  folders
    .map((f) => ({ id: f.id, label: folderPath(folders, f.id).map((x) => x.name).join(" › ") }))
    .sort((a, b) => a.label.localeCompare(b.label, undefined, { numeric: true, sensitivity: "base" }));

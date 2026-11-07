import { describe, expect, it } from "vitest";
import { childFolders, folderOptions, folderPath, indexFolders, subtreeCounts, subtreeIds, type FolderRow } from "./folderPath";

const f = (id: number, name: string, parent_id: number | null = null): FolderRow => ({ id, name, parent_id });
const names = (rows: FolderRow[]) => rows.map((r) => r.name);

describe("indexFolders", () => {
  it("nests folders and sorts each level by name, numbers as numbers", () => {
    const index = indexFolders([f(1, "Prod"), f(2, "web10", 1), f(3, "web9", 1), f(4, "Lab")]);
    expect(names(childFolders(index, null))).toEqual(["Lab", "Prod"]);
    expect(names(childFolders(index, 1))).toEqual(["web9", "web10"]);
    expect(childFolders(index, 4)).toEqual([]);
  });

  it("shows a folder whose parent is gone at the top level", () => {
    const index = indexFolders([f(1, "Orphan", 42)]);
    expect(index.parent.get(1)).toBeNull();
    expect(names(childFolders(index, null))).toEqual(["Orphan"]);
  });

  it("breaks a loop of parents, so no folder is unreachable", () => {
    // 1 → 2 → 3 → 1, and 4 inside 3.
    const index = indexFolders([f(1, "A", 3), f(2, "B", 1), f(3, "C", 2), f(4, "D", 3)]);
    expect(names(childFolders(index, null))).toEqual(["A", "B", "C"]);
    expect(names(childFolders(index, 3))).toEqual(["D"]);
    expect(index.parent.get(4)).toBe(3);
  });

  it("treats a folder that is its own parent as top level", () => {
    expect(indexFolders([f(1, "Self", 1)]).parent.get(1)).toBeNull();
  });
});

describe("folderPath", () => {
  const index = indexFolders([f(1, "Prod"), f(2, "EU", 1), f(3, "Web", 2)]);

  it("lists the folders outermost first", () => {
    expect(names(folderPath(index, 3))).toEqual(["Prod", "EU", "Web"]);
  });

  it("is empty for no folder or an unknown one", () => {
    expect(folderPath(index, null)).toEqual([]);
    expect(folderPath(index, 99)).toEqual([]);
  });
});

describe("subtreeCounts and subtreeIds", () => {
  const index = indexFolders([f(1, "Prod"), f(2, "EU", 1), f(3, "US", 1), f(4, "Web", 2), f(5, "Lab")]);

  it("counts what's in a folder's folders as in it too, ignoring unknown folders", () => {
    const counts = subtreeCounts(index, [4, 4, 3, null, undefined, 77, 5]);
    expect(Object.fromEntries(counts)).toEqual({ 1: 3, 2: 2, 3: 1, 4: 2, 5: 1 });
  });

  it("gives a folder and everything inside it", () => {
    expect(subtreeIds(index, 1).sort()).toEqual([1, 2, 3, 4]);
    expect(subtreeIds(index, 5)).toEqual([5]);
  });

  it("ends on a loop", () => {
    // A and B each other's parent: both shown at the top level.
    const loop = indexFolders([f(1, "A", 2), f(2, "B", 1), f(3, "C", 1)]);
    expect(subtreeIds(loop, 1).sort()).toEqual([1, 3]);
  });
});

describe("folderOptions", () => {
  it("tells two folders with the same name apart, sorted by their whole path", () => {
    const options = folderOptions([f(1, "Prod"), f(2, "Web", 1), f(3, "Lab"), f(4, "Web", 3)]);
    expect(options.map((o) => o.label)).toEqual(["Lab", "Lab › Web", "Prod", "Prod › Web"]);
    expect(options.find((o) => o.label === "Prod › Web")?.id).toBe(2);
  });
});

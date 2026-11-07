// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).
// The fork's terminal and tab helpers that decide without a page.
import { describe, expect, it } from "vitest";
import { claimGpu, gpuChanged, gpuSlotsInUse, MAX_GPU, releaseGpu, type GpuHolder } from "../../src/ui/gpuSlots";
import { reorderById } from "../../src/ui/tabDrag";
import { hasVisibleText } from "../../src/ui/terminalActivity";

describe("hasVisibleText", () => {
  it("ignores control codes: colours, a title change, cursor moves", () => {
    expect(hasVisibleText("\x1b[0m\x1b]0;user@host: ~\x07\x1b[2J\x1b[H")).toBe(false);
    expect(hasVisibleText("\x1b]2;title\x1b\\\r\n")).toBe(false);
  });

  it("needs a few visible characters, not a lone prompt redraw", () => {
    expect(hasVisibleText("$ ")).toBe(false);
    expect(hasVisibleText(" ok\r\n")).toBe(false);
    expect(hasVisibleText("\x1b[32mdone\x1b[0m\r\n")).toBe(true);
  });
});

describe("reorderById", () => {
  const tabs = ["a", "b", "c", "d"].map((id) => ({ id }));
  const ids = (list: { id: string }[]) => list.map((t) => t.id).join("");

  it("moves a tab before or after another", () => {
    expect(ids(reorderById(tabs, "a", "c", false))).toBe("bacd");
    expect(ids(reorderById(tabs, "a", "c", true))).toBe("bcad");
    expect(ids(reorderById(tabs, "d", "a", false))).toBe("dabc");
  });

  it("leaves the list as it is for a drop on itself or an unknown tab", () => {
    expect(reorderById(tabs, "b", "b", true)).toBe(tabs);
    expect(reorderById(tabs, "x", "b", true)).toBe(tabs);
    expect(reorderById(tabs, "b", "x", true)).toBe(tabs);
  });
});

describe("gpuSlots", () => {
  const holder = (onScreen: boolean) => {
    const h = { screen: onScreen, evicted: 0, retried: 0 } as { screen: boolean; evicted: number; retried: number } & GpuHolder;
    h.onScreen = () => h.screen;
    h.evict = () => { h.evicted++; };
    h.retry = () => { h.retried++; };
    return h;
  };

  it("hands out at most MAX_GPU contexts, takes one back from a terminal off screen, and retries waiting ones", () => {
    const held = Array.from({ length: MAX_GPU }, () => holder(true));
    for (const h of held) expect(claimGpu(h)).toBe(true);
    expect(claimGpu(held[0])).toBe(true); // already holding: no change

    // All on screen: the next one waits.
    const late = holder(true);
    expect(claimGpu(late)).toBe(false);
    expect(gpuSlotsInUse()).toEqual({ holding: MAX_GPU, waiting: 1 });

    // One goes off screen: the next claim takes its context.
    held[3].screen = false;
    const next = holder(true);
    expect(claimGpu(next)).toBe(true);
    expect(held[3].evicted).toBe(1);

    // One closes: the terminal on screen that waited is told to try again.
    releaseGpu(held[0]);
    expect(late.retried).toBe(1);
    expect(claimGpu(late)).toBe(true);

    // Nothing waits now: a change wakes nobody.
    gpuChanged();
    expect(late.retried).toBe(1);
    for (const h of [...held, late, next]) releaseGpu(h);
    expect(gpuSlotsInUse()).toEqual({ holding: 0, waiting: 0 });
  });
});

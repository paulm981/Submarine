// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).
// Small helpers of the fork's own, one describe each.
import { describe, expect, it } from "vitest";
import { visibleName } from "../../src/util/fileName";
import { clampInfoFontSize, DEFAULT_INFO_FONT_SIZE, MAX_INFO_FONT_SIZE, MIN_INFO_FONT_SIZE } from "../../src/util/infoFont";
import { jumpHostNote } from "../../src/util/jumpHostNote";
import { rangePick } from "../../src/util/rangePick";
import { sessionDot } from "../../src/util/sessionStatus";

describe("visibleName", () => {
  it("drops a right-to-left override, so a name can't pass for another", () => {
    // Shown as "Invoice_exe.pdf" with the override kept.
    expect(visibleName("Invoice_‮fdp.exe")).toBe("Invoice_fdp.exe");
  });

  it("drops zero-width, tag and separator characters, and keeps the rest", () => {
    expect(visibleName("a​b⁠c\u{E0041}d e️f")).toBe("abcdef");
    expect(visibleName("résumé 2024 – 日本.txt")).toBe("résumé 2024 – 日本.txt");
  });
});

describe("rangePick", () => {
  const order = ["a", "b", "c", "d", "e"];

  it("picks from the anchor to the item, either way, on top of what was picked before", () => {
    expect(rangePick(order, "b", "d", ["e"])).toEqual(["e", "b", "c", "d"]);
    expect(rangePick(order, "d", "b", [])).toEqual(["b", "c", "d"]);
    expect(rangePick(order, "c", "c", ["c"])).toEqual(["c"]);
  });

  it("is null when the anchor or the item isn't in the list", () => {
    expect(rangePick(order, "x", "b", [])).toBeNull();
    expect(rangePick(order, "a", "x", [])).toBeNull();
  });
});

describe("jumpHostNote", () => {
  const servers = [
    { id: 1, name: "bastion" },
    { id: 2, name: "jump2" },
    { id: 3, name: "web", jump_host_id: 1 },
    { id: 4, name: "db", jump_host_id: 1 },
    { id: 5, name: "cache", jump_host_id: 2 },
  ];

  it("is empty when no server that stays goes through one being deleted", () => {
    expect(jumpHostNote(servers, new Set([5]))).toBe("");
    expect(jumpHostNote(null, new Set([1]))).toBe("");
    // Deleted together with its jump host: nothing to warn about.
    expect(jumpHostNote(servers, new Set([1, 3, 4]))).toBe("");
  });

  it("names the jump host and counts the servers behind it", () => {
    expect(jumpHostNote(servers, new Set([1]))).toBe(
      " 2 servers use bastion as their jump host. They won't connect until you pick another jump host or choose to connect directly.",
    );
    expect(jumpHostNote(servers, new Set([1, 4]))).toBe(
      " 1 server uses bastion as its jump host. It won't connect until you pick another jump host or choose to connect directly.",
    );
    expect(jumpHostNote(servers, new Set([1, 2]))).toContain(" 3 servers use bastion or jump2 as their jump host.");
  });
});

describe("clampInfoFontSize", () => {
  it("keeps the Info panel's size within its limits, rounded", () => {
    expect(clampInfoFontSize(13)).toBe(13);
    expect(clampInfoFontSize("14")).toBe(14);
    expect(clampInfoFontSize(12.6)).toBe(13);
    expect(clampInfoFontSize(2)).toBe(MIN_INFO_FONT_SIZE);
    expect(clampInfoFontSize("999")).toBe(MAX_INFO_FONT_SIZE);
  });

  it("falls back to the default for anything that isn't a number", () => {
    for (const v of [null, undefined, "", "big", NaN, Infinity, {}]) expect(clampInfoFontSize(v)).toBe(DEFAULT_INFO_FONT_SIZE);
  });
});

describe("sessionDot", () => {
  it("lets a prompt waiting for you outrank the connection state", () => {
    expect(sessionDot("connected", true).title).toBe("Waiting for you — open this tab to answer");
    expect(sessionDot("connected", true).tone).toContain("animate-pulse");
  });

  it("pulses only while connecting, and shows an unknown state as it is", () => {
    expect(sessionDot("connected", false).tone).not.toContain("animate");
    expect(sessionDot("connecting", false).tone).toContain("animate");
    expect(sessionDot("failed", false).tone).toBe(sessionDot("disconnected", false).tone);
    expect(sessionDot("paused", false)).toEqual({ tone: "bg-zinc-500", title: "paused" });
  });
});

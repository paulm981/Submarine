// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).

import { describe, expect, it } from "vitest";
import { buildMatcher, HIGHLIGHT_MINE, HIGHLIGHT_NONE, normalizeRules, ruleProblem, type CustomRule } from "../../src/ui/highlight";

const rule = (over: Partial<CustomRule>): CustomRule => ({
  pattern: "error", kind: "word", matchCase: false, color: "blue", bold: false, underline: false, enabled: true, ...over,
});

describe("normalizeRules", () => {
  it("keeps rules this build can use and fills in what's missing", () => {
    const rules = normalizeRules([
      { id: 3, pattern: "boom", kind: "text", color: "green", bold: 1 },
      { id: "x", pattern: "ok", kind: "word", color: "ultraviolet", enabled: false },
    ]);
    expect(rules).toEqual([
      { id: 3, pattern: "boom", kind: "text", matchCase: false, color: "green", bold: true, underline: false, enabled: true },
      { id: undefined, pattern: "ok", kind: "word", matchCase: false, color: "red", bold: false, underline: false, enabled: false },
    ]);
  });

  it("drops a kind from a newer build, a rule without text, and anything that isn't a list", () => {
    expect(normalizeRules([{ pattern: "x", kind: "glob" }, { kind: "word" }, null, 7])).toEqual([]);
    expect(normalizeRules({ pattern: "x", kind: "word" })).toEqual([]);
    expect(normalizeRules(undefined)).toEqual([]);
  });
});

describe("ruleProblem", () => {
  it("asks for text, and refuses text over 500 characters", () => {
    expect(ruleProblem({ pattern: "   ", kind: "word", matchCase: false })).toBe("Type the text to highlight.");
    expect(ruleProblem({ pattern: "a".repeat(501), kind: "text", matchCase: false })).toMatch(/too long/);
    expect(ruleProblem({ pattern: "a".repeat(500), kind: "text", matchCase: false })).toBeNull();
  });

  it("refuses an expression that doesn't compile, or matches empty text", () => {
    expect(ruleProblem({ pattern: "(", kind: "regex", matchCase: false })).toBe("That isn't a valid regular expression.");
    expect(ruleProblem({ pattern: "x*", kind: "regex", matchCase: false })).toMatch(/matches empty text/);
    // Only as a regular expression: the same text as a word is fine.
    expect(ruleProblem({ pattern: "(", kind: "word", matchCase: false })).toBeNull();
  });

  it("refuses an expression that takes exponential time, and accepts a plain one", () => {
    expect(ruleProblem({ pattern: "^(a+)+$", kind: "regex", matchCase: false })).toMatch(/too slow/);
    expect(ruleProblem({ pattern: "disk \\d+% full", kind: "regex", matchCase: false })).toBeNull();
  });
});

describe("buildMatcher", () => {
  it("highlights nothing with the None set, or with only rules that are off", () => {
    expect(buildMatcher(HIGHLIGHT_NONE, [rule({})])).toBeNull();
    expect(buildMatcher(HIGHLIGHT_MINE, [rule({ enabled: false })])).toBeNull();
    // Regular-expression rules are kept but not used in this build.
    expect(buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern: "err\\w+" })])).toBeNull();
  });

  it("matches a word whole, in any case unless asked", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [rule({ pattern: "fail" })])!;
    expect(m.find("FAIL failed fail")).toEqual([[0, 4, { color: "blue", bold: false, underline: false }], [12, 16, expect.anything()]]);
    const exact = buildMatcher(HIGHLIGHT_MINE, [rule({ pattern: "fail", matchCase: true })])!;
    expect(exact.find("FAIL fail").map(([s, e]) => [s, e])).toEqual([[5, 9]]);
  });

  it("matches text with special characters literally", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [rule({ pattern: "a.b(1)", kind: "text" })])!;
    expect(m.find("aXb(1) a.b(1)").map(([s, e]) => [s, e])).toEqual([[7, 13]]);
  });

  it("lets the user's own rule win over the set's where both match", () => {
    const m = buildMatcher("standard", [rule({ pattern: "error", color: "cyan" })])!;
    const spans = m.find("an error and a warning");
    expect(spans.map(([s, e, st]) => [s, e, st.color])).toEqual([[3, 8, "cyan"], [15, 22, "bright-yellow"]]);
  });
});

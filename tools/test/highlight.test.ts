// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).

import { describe, expect, it } from "vitest";
import { buildMatcher, grayBrights, HighlightStream, HIGHLIGHT_MINE, HIGHLIGHT_NONE, normalizeRules, ruleProblem, type CustomRule } from "../../src/ui/highlight";

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
    expect(ruleProblem({ pattern: "(", kind: "regex", matchCase: false })).toMatch(/isn't a regular expression/);
    expect(ruleProblem({ pattern: "x*", kind: "regex", matchCase: false })).toMatch(/matches empty text/);
    // Only as a regular expression: the same text as a word is fine.
    expect(ruleProblem({ pattern: "(", kind: "word", matchCase: false })).toBeNull();
  });

  it("refuses back-references and lookarounds, which RE2 can't run, and accepts the rest", () => {
    expect(ruleProblem({ pattern: "(a)\\1", kind: "regex", matchCase: false })).toMatch(/Back-references/);
    expect(ruleProblem({ pattern: "foo(?=bar)", kind: "regex", matchCase: false })).toMatch(/lookarounds/);
    // Nested repeats are fine: RE2 runs them in time that grows with the line, not exponentially.
    expect(ruleProblem({ pattern: "^(a+)+$", kind: "regex", matchCase: false })).toBeNull();
    expect(ruleProblem({ pattern: "disk \\d+% full", kind: "regex", matchCase: false })).toBeNull();
  });
});

describe("buildMatcher", () => {
  it("highlights nothing with the None set, or with only rules that are off", () => {
    expect(buildMatcher(HIGHLIGHT_NONE, [rule({})])).toBeNull();
    expect(buildMatcher(HIGHLIGHT_MINE, [rule({ enabled: false })])).toBeNull();
  });

  it("matches a regular expression, in any case unless asked", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern: "err\\w+" })])!;
    expect(m.find("ERROR here err42").map(([s, e]) => [s, e])).toEqual([[0, 5], [11, 16]]);
    const exact = buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern: "err\\w+", matchCase: true })])!;
    expect(exact.find("ERROR here err42").map(([s, e]) => [s, e])).toEqual([[11, 16]]);
  });

  it("counts a regular expression's places as the line's own, past an emoji", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern: "err\\w+" })])!;
    const line = "\u{1F600} x ERROR here err42";
    expect(m.find(line).map(([s, e]) => line.slice(s, e))).toEqual(["ERROR", "err42"]);
  });

  it("runs an expression that would hang a backtracking engine in good time", () => {
    const line = "a".repeat(4096) + "!";
    for (const pattern of ["(a*)*b", "(a+)+$", "(.*a){12}x", "(\\w+\\s?)+=$"]) {
      const m = buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern })])!;
      const t0 = performance.now();
      m.find(line);
      expect(performance.now() - t0, pattern).toBeLessThan(500);
    }
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

describe("the stream follows what programs set (#49)", () => {
  // How many times `fail` comes out highlighted, from `text` in one batch.
  const lit = (text: string): number => {
    const st = new HighlightStream(() => {});
    st.setMatcher(buildMatcher(HIGHLIGHT_MINE, [rule({ pattern: "fail" })]), false);
    const out = st.push(new TextEncoder().encode(text));
    return (out.match(/\x1b\[[0-9;]+mfail\x1b\[39/g) ?? []).length;
  };

  it("highlights plain text", () => {
    expect(lit("fail")).toBe(1);
    expect(lit("\x1b[31mfail")).toBe(0);
  });

  it("ends an OSC with BEL, and a DCS, SOS, PM or APC only with ST", () => {
    expect(lit("\x1b]0;title\x07fail")).toBe(1);
    expect(lit("\x1bPq data\x07fail")).toBe(0);
    expect(lit("\x1bPq data\x07fail\x1b\\fail")).toBe(1);
    expect(lit("\x1b_apc\x07fail\x1b\\ fail")).toBe(1);
    expect(lit("\x9d0;t\x07fail")).toBe(1);
    expect(lit("\x90q\x07fail\x9cfail")).toBe(1);
  });

  it("lets a C1 control inside a sequence act as it does anywhere", () => {
    // ST inside a CSI ends it: the colour it was setting never applies.
    expect(lit("\x1b[31\x9cfail")).toBe(1);
    // A C1 CSI inside an escape starts a new sequence.
    expect(lit("\x1b(\x9b31mfail")).toBe(0);
  });

  it("puts attributes back with ESC 8, CSI u and on leaving the alternate screen", () => {
    expect(lit("\x1b[31m\x1b7\x1b[0mfail\x1b8fail")).toBe(1);
    expect(lit("\x1b[31m\x1b[s\x1b[0mfail\x1b[ufail")).toBe(1);
    expect(lit("\x1b[31m\x1b[?1049h\x1b[0mtop\x1b[?1049lfail")).toBe(0);
    // ESC 8 with nothing saved: the defaults.
    expect(lit("\x1b[31m\x1b8fail")).toBe(1);
  });
});

describe("schemes whose bright colors are grays (#49)", () => {
  const solarized = { brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1" };
  it("finds Solarized's gray brights, and none in a colorful scheme", () => {
    expect([...grayBrights(solarized)].sort()).toEqual(["bright-blue", "bright-cyan", "bright-green", "bright-yellow"]);
    expect(grayBrights({ brightRed: "#e06c75", brightGreen: "#98c379", brightYellow: "#e5c07b", brightBlue: "#61afef", brightMagenta: "#c678dd", brightCyan: "#56b6c2" }).size).toBe(0);
    expect(grayBrights({}).size).toBe(0);
  });

  it("draws a gray bright in its normal color", () => {
    const st = new HighlightStream(() => {});
    st.setMatcher(buildMatcher(HIGHLIGHT_MINE, [rule({ pattern: "ok", color: "bright-green" })]), false);
    expect(st.push(new TextEncoder().encode("ok"))).toContain("\x1b[92mok");
    st.setScheme(solarized);
    expect(st.push(new TextEncoder().encode("ok"))).toContain("\x1b[32mok");
  });
});

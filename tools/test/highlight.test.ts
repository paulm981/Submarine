// The page's tests. From the repository root: npm --prefix tools/test ci once,
// then npm --prefix tools/test test (type check, then vitest).

import { describe, expect, it, vi } from "vitest";
import { buildMatcher, grayBrights, HighlightStream, HIGHLIGHT_MINE, HIGHLIGHT_NONE, normalizeRules, onSlowRule, ruleProblem, type CustomRule } from "../../src/ui/highlight";

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

  it("says what's wrong with an expression RE2 can't run", () => {
    const problem = (pattern: string) => ruleProblem({ pattern, kind: "regex", matchCase: false });
    expect(problem("(")).toBe("That isn't a regular expression Submarine can use: missing closing ): `(`");
    expect(ruleProblem({ pattern: "(", kind: "regex", matchCase: true })).toBe("That isn't a regular expression Submarine can use: missing closing ): `(`");
    expect(problem("(a)\\1")).toBe("Back-references (\\1) aren't supported.");
    for (const p of ["foo(?=bar)", "foo(?!bar)", "(?<=u=)\\w+", "(?<!x)y"]) expect(problem(p), p).toMatch(/^Lookarounds/);
    expect(problem("\\u2713")).toMatch(/\\x\{2713\}/);
    // Only as a regular expression: the same text as a word is fine.
    expect(ruleProblem({ pattern: "(", kind: "word", matchCase: false })).toBeNull();
  });

  it("refuses an expression that can match empty text, even where it doesn't on empty text itself", () => {
    for (const p of ["x*", "\\b", "\\b|foo", "\\B", "$"]) {
      expect(ruleProblem({ pattern: p, kind: "regex", matchCase: false }), p).toMatch(/can match empty text/);
    }
  });

  it("refuses an expression too big to run on every line, and takes big ordinary ones", () => {
    expect(ruleProblem({ pattern: "\\pL{1000}x", kind: "regex", matchCase: false })).toMatch(/too big/);
    // A full IPv6 address: about 500 instructions, under the limit.
    const hex = "[0-9a-fA-F]{1,4}";
    const ipv6 = [`(?:${hex}:){7}${hex}`, `(?:${hex}:){1,7}:`, `(?:${hex}:){1,6}:${hex}`, `(?:${hex}:){1,5}(?::${hex}){1,2}`,
      `(?:${hex}:){1,4}(?::${hex}){1,3}`, `(?:${hex}:){1,3}(?::${hex}){1,4}`, `(?:${hex}:){1,2}(?::${hex}){1,5}`, `${hex}:(?::${hex}){1,6}`].join("|");
    expect(ruleProblem({ pattern: ipv6, kind: "regex", matchCase: false })).toBeNull();
    // Nested repeats are fine: RE2 doesn't backtrack.
    expect(ruleProblem({ pattern: "^(a+)+$", kind: "regex", matchCase: false })).toBeNull();
    expect(ruleProblem({ pattern: "disk \\d+% full", kind: "regex", matchCase: false })).toBeNull();
  });
});

describe("regular-expression rules, on RE2 (#49)", () => {
  const regex = (pattern: string, over: Partial<CustomRule> = {}) => rule({ kind: "regex", pattern, ...over });
  const at = (spans: [number, number, unknown][]) => spans.map(([s, e]) => [s, e]);

  it("matches in any case unless asked, folding letters beyond ASCII too", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [regex("err\\w+")])!;
    expect(at(m.find("ERROR here err42"))).toEqual([[0, 5], [11, 16]]);
    expect(at(buildMatcher(HIGHLIGHT_MINE, [regex("err\\w+", { matchCase: true })])!.find("ERROR here err42"))).toEqual([[11, 16]]);
    expect(at(buildMatcher(HIGHLIGHT_MINE, [regex("café")])!.find("CAFÉ"))).toEqual([[0, 4]]);
  });

  it("places matches in the text's own units, past emoji, on every line of a batch", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [regex("err\\d+")])!;
    const text = "😀 x err1 😀err22\nerr3";
    expect(m.find(text).map(([s, e]) => text.slice(s, e))).toEqual(["err1", "err22", "err3"]);
    expect(at(m.find("x\nerr1\nerr2"))).toEqual([[2, 6], [7, 11]]);
  });

  it("takes the first rule where two match, and wins over the set", () => {
    const m = buildMatcher("standard", [regex("err\\d+", { color: "red" }), regex("\\d+", { color: "green" }), regex("\\berror\\b", { color: "cyan" })])!;
    expect(m.find("err42 7 an error").map(([s, e, st]) => [s, e, st.color])).toEqual([[0, 5, "red"], [6, 7, "green"], [11, 16, "cyan"]]);
  });

  it("leaves out a rule it can't use, from another computer or an older build, and keeps the rest", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [regex("(?<=u=)\\w+"), regex("\\b"), regex("\\pL{1000}x"), rule({ pattern: "fail" })])!;
    expect(at(m.find("u=x fail"))).toEqual([[4, 8]]);
  });

  it("runs an expression that would hang a backtracking engine on the longest line it takes, and keeps it on", () => {
    const slow: string[] = [];
    onSlowRule((label) => slow.push(label));
    // 4096 characters: the longest line a user's rule is run on.
    const line = "a".repeat(4095) + "b";
    for (const pattern of ["(a*)*b", "(a+)+b", "(.*a){12}b", "(\\w+\\s?)+b"]) {
      const m = buildMatcher(HIGHLIGHT_MINE, [regex(pattern)])!;
      expect(m.find(line).length, pattern).toBe(1);
      expect(m.find(line).length, `${pattern}, again`).toBe(1);
    }
    expect(slow).toEqual([]);
    // One character more, and the line is left alone.
    expect(buildMatcher(HIGHLIGHT_MINE, [regex("a+")])!.find(line + "a")).toEqual([]);
  });

  it("stops a rule that's slow on real output partway through a batch, and keeps it off", () => {
    const slow: string[] = [];
    onSlowRule((label) => slow.push(label));
    // Just under the size limit, on a batch of long lines: about 50 ms a line.
    const pattern = "[a-z]{590}\\d";
    const batch = Array(20).fill("a".repeat(4095) + "1").join("\n");
    const m = buildMatcher(HIGHLIGHT_MINE, [regex(pattern)])!;
    const t0 = performance.now();
    m.find(batch);
    expect(performance.now() - t0).toBeLessThan(400);
    expect(slow).toEqual([pattern]);
    // Off from then on, in this matcher and in a new one.
    const short = "a".repeat(600) + "1";
    expect(m.find(short)).toEqual([]);
    expect(buildMatcher(HIGHLIGHT_MINE, [regex(pattern), rule({ pattern: "fail" })])!.find(short)).toEqual([]);
  });
});

describe("a regular expression's ^, $ and \\b follow the terminal's line (#49)", () => {
  // The text highlighted, from `batches` given one after another.
  const lit = (pattern: string, ...batches: string[]): string[] => {
    const st = new HighlightStream(() => {});
    st.setMatcher(buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern })]), false);
    const out = batches.map((b) => st.push(new TextEncoder().encode(b))).join("");
    return [...out.matchAll(/\x1b\[34m([^\x1b]*)\x1b\[39m/g)].map((m) => m[1]);
  };

  it("takes ^ as the start of the line, not of each key typed", () => {
    expect(lit("^e", "$ ", "e", "c", "h", "o", " ", "e")).toEqual([]);
    expect(lit("^e", "echo\r\n", "e\r\n")).toEqual(["e", "e"]);
  });

  it("takes ^ as the start of the line, not where a program's color ends", () => {
    expect(lit("^\\d+:", "\x1b[35mf.rs\x1b[m\x1b[36m:\x1b[m42:x\r\n")).toEqual([]);
    expect(lit("^\\d+:", "42:x\r\n")).toEqual(["42:"]);
    // Colors that only bolden or reset don't move the start.
    expect(lit("^\\d+:", "\x1b[1m\x1b[0m42:x\r\n")).toEqual(["42:"]);
  });

  it("takes $ as the end of the line, not where a color starts or a batch ends", () => {
    expect(lit("ok$", "build ok\x1b[33m (2 warnings)\x1b[0m\r\n")).toEqual([]);
    expect(lit("ok$", "build ok\r\n")).toEqual(["ok"]);
    expect(lit("ok$", "build ok\x1b[0m\r\n")).toEqual(["ok"]);
    // Typed, one key at a time: the line may go on.
    expect(lit("ok$", "o", "k")).toEqual([]);
    // The end of a big batch waits for the rest of its line, and is told how it ends.
    vi.stubGlobal("window", { setTimeout, clearTimeout });
    expect(lit("ok$", `${"x".repeat(600)}\r\nbuild ok`, "\r\n")).toEqual(["ok"]);
    expect(lit("ok$", `${"x".repeat(600)}\r\nbuild ok`, " now\r\n")).toEqual([]);
    vi.unstubAllGlobals();
  });

  it("highlights only the part of a match on this piece of the line", () => {
    const m = buildMatcher(HIGHLIGHT_MINE, [rule({ kind: "regex", pattern: "ok." })])!;
    expect(m.find("ok\nok", [{ before: "", after: "x" }, { before: "", after: "" }]).map(([s, e]) => [s, e])).toEqual([[0, 2]]);
    expect(lit("ok.", "ok\x1b[1mx\r\n")).toEqual(["ok"]);
  });

  it("sees a word go on past a color change, and not past a cursor move", () => {
    expect(lit("\\bfail\\b", "fail\x1b[31med\x1b[0m\r\n")).toEqual([]);
    expect(lit("\\bfail\\b", "x\x1b[31m-\x1b[0mfail\r\n")).toEqual(["fail"]);
    // After the cursor moves, what was printed before isn't next to it.
    expect(lit("\\bfail", "x\x1b[5Gfail\r\n")).toEqual(["fail"]);
    expect(lit("^fail", "\r\n\x1b[Afail\r\n")).toEqual([]);
    expect(lit("\\bfail", "x\bfail\r\n")).toEqual(["fail"]);
    expect(lit("^fail", "x\x1b[Kfail\r\n")).toEqual([]);
    expect(lit("^fail", "x\rfail\r\n")).toEqual(["fail"]);
    // NEL, the C1 next line.
    expect(lit("^fail", "x\u0085fail\r\n")).toEqual(["fail"]);
  });
});

describe("buildMatcher", () => {
  it("highlights nothing with the None set, or with only rules that are off", () => {
    expect(buildMatcher(HIGHLIGHT_NONE, [rule({})])).toBeNull();
    expect(buildMatcher(HIGHLIGHT_MINE, [rule({ enabled: false })])).toBeNull();
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

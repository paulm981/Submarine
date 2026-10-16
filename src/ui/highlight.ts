// ─────────────────────────────────────────────────────────────────────────────
// Syntax highlighting: color added to what a terminal shows.
//
// A terminal's output passes through a HighlightStream on its way to xterm.
// Plain text in it (text no program has colored) is matched against a set of
// rules, and what matches is wrapped in color codes. Only the screen changes:
// what the server sent, what is copied out of the terminal and what is typed
// are all untouched.
//
// - Rules come in sets (HIGHLIGHT_SETS, our own lists) plus the user's own
//   (CustomRule, synced with the profile; src-tauri/src/highlight.rs). Their
//   own come first, so they win where both match the same text.
// - Colors are the scheme's own (red is the scheme's red), so highlights
//   suit every color scheme.
// - Full-screen programs (vim, less, top) are left alone: nothing is
//   highlighted on the alternate screen. Text a program colored itself, or
//   shows on a colored background or inverted, isn't touched either.
// - A set is chosen per device (Settings › Terminal) and per server (its edit
//   panel); a server's choice syncs with it, like its color scheme.
// ─────────────────────────────────────────────────────────────────────────────

export type HighlightColor =
  | "red" | "green" | "yellow" | "blue" | "magenta" | "cyan"
  | "bright-red" | "bright-green" | "bright-yellow" | "bright-blue" | "bright-magenta" | "bright-cyan"
  | "gray";

/** The colors a rule can use: the SGR code, and the scheme's ANSI index for
 *  previews (ansiOf in terminalThemes). */
export const HIGHLIGHT_COLORS: readonly { id: HighlightColor; name: string; sgr: number; ansi: number }[] = [
  { id: "red", name: "Red", sgr: 31, ansi: 1 },
  { id: "green", name: "Green", sgr: 32, ansi: 2 },
  { id: "yellow", name: "Yellow", sgr: 33, ansi: 3 },
  { id: "blue", name: "Blue", sgr: 34, ansi: 4 },
  { id: "magenta", name: "Magenta", sgr: 35, ansi: 5 },
  { id: "cyan", name: "Cyan", sgr: 36, ansi: 6 },
  { id: "bright-red", name: "Bright red", sgr: 91, ansi: 9 },
  { id: "bright-green", name: "Bright green", sgr: 92, ansi: 10 },
  { id: "bright-yellow", name: "Bright yellow", sgr: 93, ansi: 11 },
  { id: "bright-blue", name: "Bright blue", sgr: 94, ansi: 12 },
  { id: "bright-magenta", name: "Bright magenta", sgr: 95, ansi: 13 },
  { id: "bright-cyan", name: "Bright cyan", sgr: 96, ansi: 14 },
  { id: "gray", name: "Gray", sgr: 90, ansi: 8 },
];
const colorById = new Map(HIGHLIGHT_COLORS.map((c) => [c.id, c]));

export const isHighlightColor = (v: unknown): v is HighlightColor => typeof v === "string" && colorById.has(v as HighlightColor);

export interface HighlightStyle {
  color: HighlightColor;
  bold?: boolean;
  underline?: boolean;
}

// ── The sets ────────────────────────────────────────────────────────────────
//
// Each rule is either a list of words (matched whole, as written, Capitalized
// or in CAPITALS) or patterns. Where two rules match overlapping text the
// earlier one wins, so a phrase goes before a word in it ("administratively
// down" before "down"). Rules in brackets together are one: whichever starts
// first wins, as a comment and a string do in a script. Every rule is its
// own scan of the text: one regex of everything was four times slower.
//
// Patterns here are written to run in linear time (no nested repeats; every
// repeat bounded by what follows it), have no groups that capture, and no
// look-behind (older macOS and Linux web views can't compile it). None may
// match a line break: a batch's lines are matched in one go, joined by "\n"
// (Matcher.find), and `^` / `$` mean a line's start and end.

interface SetRule {
  words?: readonly string[];
  /** Matched as written only (SQL's keywords in capitals). */
  exact?: boolean;
  /** Several: each its own scan, in this order. */
  pattern?: string | readonly string[];
  style: HighlightStyle;
}

type Layer = SetRule | readonly SetRule[];

export interface HighlightSet {
  id: string;
  name: string;
  description: string;
  rules: readonly Layer[];
}

const FAILURE: SetRule = {
  words: [
    "no such file or directory", "permission denied", "connection refused", "connection reset", "connection timed out",
    "segmentation fault", "core dumped", "out of memory", "broken pipe", "timed out", "not found", "unable to", "access denied",
    "errors", "error", "err", "failures", "failure", "failed", "fails", "fail", "fatal", "critical", "crit", "emergency", "emerg",
    "panicked", "panic", "exception", "traceback", "aborted", "abort", "denied", "refused", "forbidden", "unauthorized",
    "invalid", "corrupted", "corrupt", "unreachable", "segfault", "killed", "cannot", "can't", "couldn't", "inactive", "dead",
  ],
  style: { color: "bright-red", bold: true },
};
const WARNING: SetRule = {
  words: ["warnings", "warning", "warn", "deprecated", "caution", "retrying", "retry", "degraded", "skipped", "expired", "not ready"],
  style: { color: "bright-yellow" },
};
const SUCCESS: SetRule = {
  words: [
    "successfully", "successful", "success", "succeeded", "okay", "ok", "passed", "done", "completed", "complete",
    "active", "running", "enabled", "started", "listening", "established", "connected", "accepted", "healthy", "ready", "online",
  ],
  style: { color: "bright-green" },
};
const OK_TAG: SetRule = { pattern: String.raw`\[ {0,3}(?:OK|ok) {0,3}\]`, style: { color: "bright-green", bold: true } };
const QUIET: SetRule = { words: ["debug", "trace", "verbose"], style: { color: "gray" } };
const LAST_LOGIN: SetRule = { words: ["last failed login"], style: { color: "bright-red", bold: true } };
const LAST_LOGIN_OK: SetRule = { words: ["last login"], style: { color: "cyan", bold: true } };

const OCTET = String.raw`(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)`;
const HEX4 = "[0-9a-fA-F]{1,4}";
const ADDRESSES: SetRule = {
  pattern: [
    // IPv4, with a prefix length or a port.
    String.raw`\b${OCTET}(?:\.${OCTET}){3}(?:\/(?:3[0-2]|[12]?\d)|:\d{1,5})?(?!\.?\d)`,
    // IPv6: all eight groups, or shortened with "::" after at least one.
    String.raw`\b${HEX4}(?:(?::${HEX4}){7}|(?::${HEX4}){0,6}::(?:${HEX4}(?::${HEX4}){0,6})?(?:\/\d{1,3})?)(?![\w:])`,
    String.raw`::1\b`,
    // MAC addresses: 00:1a:2b:3c:4d:5e, 00-1a-2b-…
    String.raw`\b[0-9a-fA-F]{2}(?::[0-9a-fA-F]{2}){5}\b`,
    String.raw`\b[0-9a-fA-F]{2}(?:-[0-9a-fA-F]{2}){5}\b`,
  ],
  style: { color: "cyan" },
};
const URLS: SetRule = {
  pattern: String.raw`\b(?:https?|ftp|sftp|ssh|file):\/\/[^\s"'<>]{0,2048}[^\s"'<>.,;:)\]}]`,
  style: { color: "blue", underline: true },
};
const MONTH = "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";
const TIME = String.raw`(?:[01]\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:[.,]\d{1,9})?)?`;
const DATES: SetRule = {
  pattern: [
    // 2026-10-16, 2026-10-16T01:02:03.456Z, 2026-10-16 01:02:03 +0200
    String.raw`\b\d{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\d|3[01])(?:[T ]${TIME}(?:Z|[+-]\d{2}:?\d{2})?)?(?![\w:])`,
    // Oct 16 01:02:03 (syslog), Oct 16 2026
    String.raw`\b${MONTH} {1,2}\d{1,2}(?: ${TIME}| \d{4})\b`,
    // 01:02:03 on its own
    String.raw`\b${TIME}(?![\w:])`,
  ],
  style: { color: "magenta" },
};

const STANDARD_RULES: readonly Layer[] = [LAST_LOGIN, LAST_LOGIN_OK, URLS, ADDRESSES, DATES, OK_TAG, FAILURE, WARNING, SUCCESS, QUIET];

// Shell scripts (a `cat` of one, or a script printing itself as it runs).
const SHELL_RULES: readonly SetRule[] = [
  { pattern: String.raw`(?:^|[ \t])#.*`, style: { color: "gray" } },
  { pattern: String.raw`"(?:[^"\\\n]|\\.){0,512}"|'[^'\n]{0,512}'`, style: { color: "green" } },
  { pattern: String.raw`\$(?:\{[^}\s]{1,128}\}|[A-Za-z_]\w{0,127}|[0-9#?$!@*-])`, style: { color: "cyan" } },
  {
    words: [
      "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "select",
      "return", "exit", "break", "continue", "local", "export", "readonly", "declare", "unset", "shift", "trap", "source", "eval", "exec",
    ],
    exact: true,
    style: { color: "magenta", bold: true },
  },
];

// Routers and switches: Cisco-style configs and `show` output, Juniper's
// interface names too.
const NETWORK_RULES: readonly SetRule[] = [
  { pattern: String.raw`^ {0,8}!.*`, style: { color: "gray" } },
  {
    pattern: [
      // GigabitEthernet0/1, Gi0/1, Port-channel 10, Vlan100, Te1/0/1.200
      String.raw`\b(?:(?:(?:Fast|Gigabit|TwoGigabit|FiveGigabit|TenGigabit|TwentyFiveGig|FortyGigabit|HundredGig)?Ethernet|Port-channel|Bundle-Ether|Loopback|Tunnel|Serial|Vlan|mgmt|Mgmt|Null|nve|BDI) ?|(?:Fa|Gi|Te|Tw|Fo|Hu|Eth|Po|Lo|Tu|Se|BE))\d{1,4}(?:\/\d{1,4}){0,3}(?:\.\d{1,5})?(?::\d{1,3})?\b`,
      // Juniper: ge-0/0/0.0, ae0, lo0.0, irb.100
      String.raw`\b(?:ge|xe|et|fe)-\d{1,3}\/\d{1,3}\/\d{1,3}(?::\d{1,3})?(?:\.\d{1,5})?\b`,
      String.raw`\b(?:ae|lo|em|fxp|me|reth)\d{1,4}(?:\.\d{1,5})?\b`,
      String.raw`\birb(?:\.\d{1,5})?\b`,
    ],
    style: { color: "bright-cyan" },
  },
  // Cisco's way of writing a MAC address: 001a.2b3c.4d5e (always lower case,
  // which also makes this scan half as many words).
  { pattern: String.raw`\b[0-9a-f]{4}\.[0-9a-f]{4}\.[0-9a-f]{4}\b`, style: { color: "cyan" } },
  { words: ["administratively down", "err-disabled", "errdisabled"], style: { color: "bright-yellow", bold: true } },
  { words: ["no shutdown"], style: { color: "bright-green" } },
  { words: ["down", "notconnect", "deny", "blocking", "blocked", "shutdown"], style: { color: "bright-red", bold: true } },
  { words: ["up", "permit", "forwarding"], style: { color: "bright-green" } },
  {
    pattern: String.raw`^ {0,8}(?:interface|router|vlan|hostname|ip|ipv6|description|switchport|spanning-tree|access-list|route-map|prefix-list|line|snmp-server|ntp|logging|aaa|username|enable|banner|channel-group|neighbor|network|redistribute|service|crypto|class-map|policy-map|object-group|vrf|address-family|standby|vrrp|track)\b`,
    style: { color: "blue", bold: true },
  },
];

// Database consoles (psql, mysql, sqlplus): keywords in capitals, and
// phrases that are only ever SQL in lower case too.
const SQL_RULES: readonly SetRule[] = [
  { pattern: String.raw`(?:^|[ \t])--(?:[ \t].*|$)`, style: { color: "gray" } },
  { pattern: String.raw`'(?:[^'\\\n]|\\.|''){0,512}'`, style: { color: "green" } },
  {
    words: [
      "is not null", "is null", "not null", "primary key", "foreign key", "group by", "order by", "partition by",
      "inner join", "left join", "right join", "full join", "cross join", "left outer join", "right outer join",
      "insert into", "delete from", "create table", "alter table", "drop table", "create index", "drop index", "create view",
      "select", "returning",
    ],
    style: { color: "blue", bold: true },
  },
  {
    words: [
      "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "NULL", "IS", "IN", "EXISTS", "BETWEEN", "LIKE", "ILIKE", "AS", "ON",
      "JOIN", "INNER", "LEFT", "RIGHT", "FULL", "OUTER", "CROSS", "NATURAL", "USING", "GROUP", "ORDER", "BY", "HAVING",
      "LIMIT", "OFFSET", "FETCH", "UNION", "ALL", "DISTINCT", "INTERSECT", "EXCEPT", "INSERT", "INTO", "VALUES", "UPDATE",
      "SET", "DELETE", "TRUNCATE", "CREATE", "ALTER", "DROP", "TABLE", "VIEW", "INDEX", "SEQUENCE", "SCHEMA", "DATABASE",
      "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "CHECK", "DEFAULT", "CONSTRAINT", "CASCADE", "GRANT", "REVOKE",
      "BEGIN", "COMMIT", "ROLLBACK", "TRANSACTION", "CASE", "WHEN", "THEN", "ELSE", "END", "RETURNING", "WITH", "RECURSIVE",
      "ASC", "DESC", "TRUE", "FALSE", "EXPLAIN", "ANALYZE",
    ],
    exact: true,
    style: { color: "blue", bold: true },
  },
  { pattern: String.raw`\b\d{1,20}(?:\.\d{1,20})?\b`, style: { color: "yellow" } },
];

export const HIGHLIGHT_NONE = "none";
export const HIGHLIGHT_MINE = "mine";
export const DEFAULT_HIGHLIGHT = "standard";

export const HIGHLIGHT_SETS: readonly HighlightSet[] = [
  { id: HIGHLIGHT_NONE, name: "None", description: "No highlighting.", rules: [] },
  {
    id: "standard",
    name: "Standard",
    description: "Errors in red, warnings in yellow, OK and success in green; addresses, links, dates and \"Last login\".",
    rules: STANDARD_RULES,
  },
  {
    id: "shell",
    name: "Shell script",
    description: "Standard, plus a script's comments, strings, $variables and keywords.",
    // Comments and strings first, so a word inside one stays its color.
    rules: [[SHELL_RULES[0], SHELL_RULES[1]], ...STANDARD_RULES, ...SHELL_RULES.slice(2)],
  },
  {
    id: "network",
    name: "Network devices",
    description: "Standard, plus interface names, up/down and permit/deny, and config keywords (Cisco-style, Juniper names).",
    rules: [NETWORK_RULES[0], ...STANDARD_RULES, ...NETWORK_RULES.slice(1)],
  },
  {
    id: "sql",
    name: "SQL",
    description: "Standard, plus SQL keywords (in capitals, and phrases like \"order by\"), strings, numbers and comments.",
    rules: [[SQL_RULES[0], SQL_RULES[1]], ...STANDARD_RULES, ...SQL_RULES.slice(2)],
  },
  { id: HIGHLIGHT_MINE, name: "Only my rules", description: "Just the rules you add below.", rules: [] },
];
const setById = new Map(HIGHLIGHT_SETS.map((s) => [s.id, s]));

export const highlightSetById = (id: string | null | undefined): HighlightSet =>
  (id && setById.get(id)) || setById.get(DEFAULT_HIGHLIGHT)!;

// ── The user's own rules ────────────────────────────────────────────────────

/** How a rule's text matches: as a whole word, anywhere, or as a regular
 *  expression. */
export type RuleKind = "word" | "text" | "regex";

export interface CustomRule {
  id?: number;
  pattern: string;
  kind: RuleKind;
  matchCase: boolean;
  color: HighlightColor;
  bold: boolean;
  underline: boolean;
  enabled: boolean;
}

/** Rules as highlight_rules_list returns them, minus any this build can't
 *  use (a kind from a newer one); a color it doesn't know becomes red. */
export function normalizeRules(raw: unknown): CustomRule[] {
  if (!Array.isArray(raw)) return [];
  const out: CustomRule[] = [];
  for (const r of raw) {
    if (!r || typeof r.pattern !== "string") continue;
    if (r.kind !== "word" && r.kind !== "text" && r.kind !== "regex") continue;
    out.push({
      id: typeof r.id === "number" ? r.id : undefined,
      pattern: r.pattern,
      kind: r.kind,
      matchCase: !!r.matchCase,
      color: isHighlightColor(r.color) ? r.color : "red",
      bold: !!r.bold,
      underline: !!r.underline,
      enabled: r.enabled !== false,
    });
  }
  return out;
}

const escapeRegex = (s: string) => s.replace(/[.*+?^${}()|[\]\\\/]/g, "\\$&");

/** Words go whole: a boundary on each side that starts or ends with a
 *  letter or digit (a boundary next to "-" would never match). */
const wholeWord = (escaped: string, raw: string) =>
  `${/^\w/.test(raw) ? "\\b" : ""}${escaped}${/\w$/.test(raw) ? "\\b" : ""}`;

const capitalized = (w: string) => w.charAt(0).toUpperCase() + w.slice(1);

/** One rule's source. Words match as written, Capitalized and in CAPITALS
 *  ("error", "Error", "ERROR"; not "eRRoR"), unless `exact`. */
function ruleSource(rule: SetRule): string {
  if (typeof rule.pattern === "string") return rule.pattern;
  if (rule.pattern) return rule.pattern.join("|");
  const forms = new Set<string>();
  for (const w of rule.words ?? []) {
    forms.add(w);
    if (!rule.exact) {
      forms.add(capitalized(w));
      forms.add(w.toUpperCase());
    }
  }
  // Longest first, so "errors" is tried before "error". Words that start and
  // end with a letter share one pair of boundaries: `\b(?:a|b)\b` scans
  // several times faster than `\ba\b|\bb\b`.
  const sorted = [...forms].sort((a, b) => b.length - a.length);
  const src = (w: string) => escapeRegex(w).replace(/ /g, " {1,3}");
  const plain = sorted.filter((w) => /^\w/.test(w) && /\w$/.test(w));
  const other = sorted.filter((w) => !plain.includes(w)).map((w) => wholeWord(src(w), w));
  return [...(plain.length ? [`\\b(?:${plain.map(src).join("|")})\\b`] : []), ...other].join("|");
}

/** Why a rule can't be used, or null when it can. A regular expression is
 *  tried on text made to trip up slow ones first: one that takes too long
 *  there would freeze the window on real output. */
export function ruleProblem(rule: Pick<CustomRule, "pattern" | "kind" | "matchCase">): string | null {
  if (!rule.pattern.trim()) return "Type the text to highlight.";
  if (rule.pattern.length > 500) return "That's too long; keep it under 500 characters.";
  if (rule.kind !== "regex") return null;
  let re: RegExp;
  try {
    re = new RegExp(rule.pattern, rule.matchCase ? "g" : "gi");
  } catch {
    return "That isn't a valid regular expression.";
  }
  if (re.test("")) return "That expression matches empty text, so it would match everywhere.";
  return slowPattern(re) ? "That expression is too slow on some text (it would freeze the window). Try a simpler one." : null;
}

/** True when `re` takes more than a few ms on short text built from its own
 *  characters: the usual sign of one whose time grows exponentially with the
 *  length of a line. Lengths grow a little at a time, so the try that shows
 *  it is itself still short. */
function slowPattern(re: RegExp): boolean {
  const chars = new Set(["a", "0", " ", ".", "-", "_", "x"]);
  for (const c of re.source) if (/[\w .:\/@-]/.test(c)) chars.add(c);
  const tails = ["\u0001", "!", ""];
  const probe = new RegExp(re.source, re.flags.replace("g", ""));
  for (const n of [6, 10, 14, 18, 22, 26, 30, 36, 42, 50, 60]) {
    for (const c of chars) {
      for (const tail of tails) {
        const text = c.repeat(n) + tail;
        if (slowOn(probe, text) && slowOn(probe, text) && slowOn(probe, text)) return true;
      }
    }
  }
  return false;
}

/** Over 3 ms on `text`. Asked three times before a pattern is called slow,
 *  so a pause for garbage collection can't make a good one look bad. */
function slowOn(re: RegExp, text: string): boolean {
  const t0 = performance.now();
  re.test(text);
  return performance.now() - t0 > 3;
}

// Checked once per rule as it is (the check runs the expression ~1000 times).
const customRegexes = new Map<string, RegExp | null>();

/** A custom rule's regex, or null when it can't be used. */
function customRegex(rule: CustomRule): RegExp | null {
  if (!rule.enabled) return null;
  const key = JSON.stringify([rule.pattern, rule.kind, rule.matchCase]);
  if (!customRegexes.has(key)) customRegexes.set(key, compileCustom(rule));
  return customRegexes.get(key)!;
}

/** Regular-expression rules are off in this build: the speed check can be
 *  fooled (a fixed start, then a nested repeat), and one slow expression
 *  freezes the window on every device the rule syncs to. They're kept as
 *  they are, and come back once they can run where they can be stopped. */
export const REGEX_RULES_ON = false;

function compileCustom(rule: CustomRule): RegExp | null {
  if (rule.kind === "regex" && !REGEX_RULES_ON) return null;
  if (ruleProblem(rule)) return null;
  const flags = rule.matchCase ? "g" : "gi";
  const escaped = escapeRegex(rule.pattern);
  const source = rule.kind === "regex" ? rule.pattern : rule.kind === "word" ? wholeWord(escaped, rule.pattern) : escaped;
  try {
    return new RegExp(source, flags);
  } catch {
    return null;
  }
}

// ── Matching ────────────────────────────────────────────────────────────────

/** [start, end) of `text` in `style`. */
export type Span = [number, number, HighlightStyle];

/** A user's expression only sees lines up to this long: what makes one slow
 *  grows with the length of the text it runs on. */
const CUSTOM_MAX_TEXT = 4096;

/** Spans of `add` that don't overlap any of `taken` (both sorted, neither
 *  overlapping itself), merged in order. */
function mergeSpans(taken: Span[], add: Span[]): Span[] {
  if (!taken.length) return add;
  if (!add.length) return taken;
  const out: Span[] = [];
  let i = 0;
  for (const s of add) {
    while (i < taken.length && taken[i][1] <= s[0]) out.push(taken[i++]);
    if (i < taken.length && taken[i][0] < s[1]) continue;
    out.push(s);
  }
  while (i < taken.length) out.push(taken[i++]);
  return out;
}

interface Scanner {
  re: RegExp;
  /** The style of a match: by which capture group matched, for a set. */
  styleOf: (m: RegExpExecArray) => HighlightStyle | null;
  /** A user's rule: watched for being slow, and dropped if it is. */
  custom?: { label: string; off: boolean };
}

function scan(sc: Scanner, text: string): Span[] {
  const out: Span[] = [];
  const re = sc.re;
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const start = m.index;
    const end = start + m[0].length;
    if (end === start) {
      re.lastIndex = start + 1;
      continue;
    }
    const style = sc.styleOf(m);
    if (style) out.push([start, end, style]);
  }
  return out;
}

/** A user's rule, line by line (the lines of `text`, split at "\n"), its
 *  spans placed in `text`. */
function scanLines(sc: Scanner, text: string): Span[] {
  const custom = sc.custom!;
  const out: Span[] = [];
  const t0 = performance.now();
  let at = 0;
  while (at <= text.length) {
    let nl = text.indexOf("\n", at);
    if (nl < 0) nl = text.length;
    if (nl - at <= CUSTOM_MAX_TEXT && nl > at) {
      for (const [s, e, st] of scan(sc, text.slice(at, nl))) out.push([s + at, e + at, st]);
    }
    at = nl + 1;
  }
  // Slow on real output after all: off for the rest of the session.
  if (performance.now() - t0 > 50) {
    custom.off = true;
    slowRuleListener?.(custom.label);
  }
  return out;
}

export class Matcher {
  constructor(private readonly scanners: Scanner[]) {}

  get empty(): boolean {
    return this.scanners.length === 0;
  }

  /** The spans to color in `text`, sorted and not overlapping. `text` is
   *  lines joined by "\n"; no span crosses one. The set's rules scan it
   *  whole: one scan per rule per batch, where one per line cost more than
   *  the matching. */
  find(text: string): Span[] {
    let spans: Span[] = [];
    for (const sc of this.scanners) {
      if (sc.custom) {
        if (!sc.custom.off) spans = mergeSpans(spans, scanLines(sc, text));
      } else {
        spans = mergeSpans(spans, scan(sc, text));
      }
    }
    return spans;
  }
}

let slowRuleListener: ((label: string) => void) | null = null;
/** Told when a user's rule is turned off for being slow (for the log). */
export const onSlowRule = (f: (label: string) => void) => {
  slowRuleListener = f;
};

const compiledSets = new Map<string, Scanner[]>();

/** Capturing groups in a regex source: the match of `(?:src)|` on empty
 *  text has one entry per group, plus the match itself. */
const groupCount = (src: string) => new RegExp(`(?:${src})|`).exec("")!.length - 1;

/** A set's scans, in priority order. A pattern this web view can't compile
 *  is left out, not the set. */
function setScans(set: HighlightSet): Scanner[] {
  const cached = compiledSets.get(set.id);
  if (cached) return cached;
  const out: Scanner[] = [];
  const add = (src: string, styleOf: Scanner["styleOf"]) => {
    try {
      out.push({ re: new RegExp(src, "gm"), styleOf });
    } catch {
      /* skipped */
    }
  };
  for (const layer of set.rules) {
    if (Array.isArray(layer)) {
      // Together: each rule a group of one regex, so the one that starts
      // first wins. Group numbers allow for groups of a rule's own.
      const rules = layer as readonly SetRule[];
      const groups: { index: number; style: HighlightStyle }[] = [];
      let next = 1;
      const parts = rules.map((r) => {
        const src = ruleSource(r);
        groups.push({ index: next, style: r.style });
        next += 1 + groupCount(src);
        return `(${src})`;
      });
      add(parts.join("|"), (m) => groups.find((g) => m[g.index] !== undefined)?.style ?? null);
    } else {
      const rule = layer as SetRule;
      const style = rule.style;
      const sources = Array.isArray(rule.pattern) ? (rule.pattern as readonly string[]) : [ruleSource(rule)];
      for (const src of sources) add(src, () => style);
    }
  }
  compiledSets.set(set.id, out);
  return out;
}

/** The matcher for a set and the user's rules; null when nothing would be
 *  highlighted. */
export function buildMatcher(setId: string, custom: readonly CustomRule[]): Matcher | null {
  if (setId === HIGHLIGHT_NONE) return null;
  const scanners: Scanner[] = [];
  for (const rule of custom) {
    const re = customRegex(rule);
    if (re) {
      const style: HighlightStyle = { color: rule.color, bold: rule.bold, underline: rule.underline };
      scanners.push({ re, styleOf: () => style, custom: { label: rule.pattern, off: false } });
    }
  }
  scanners.push(...setScans(highlightSetById(setId)));
  return scanners.length ? new Matcher(scanners) : null;
}

// ── The stream ──────────────────────────────────────────────────────────────

/** What the program has set for the text it prints next. */
class Attrs {
  fg = false;
  bg = false;
  inverse = false;
  hidden = false;
  bold = false;
  dim = false;
  underline = false;

  reset() {
    this.fg = this.bg = this.inverse = this.hidden = this.bold = this.dim = this.underline = false;
  }

  /** Text nobody colored: the only kind that's highlighted. */
  get plain(): boolean {
    return !this.fg && !this.bg && !this.inverse && !this.hidden;
  }

  /** SGR parameters (`ESC [ … m`). */
  apply(params: string) {
    if (params === "") {
      this.reset();
      return;
    }
    const parts = params.split(";");
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i];
      const colon = p.indexOf(":");
      const n = parseInt(colon >= 0 ? p.slice(0, colon) : p, 10) || 0;
      if (n === 0) this.reset();
      else if (n === 1) this.bold = true;
      else if (n === 2) this.dim = true;
      else if (n === 22) this.bold = this.dim = false;
      else if (n === 4) this.underline = colon < 0 || p.slice(colon + 1) !== "0";
      else if (n === 21) this.underline = true;
      else if (n === 24) this.underline = false;
      else if (n === 7) this.inverse = true;
      else if (n === 27) this.inverse = false;
      else if (n === 8) this.hidden = true;
      else if (n === 28) this.hidden = false;
      else if ((n >= 30 && n <= 37) || (n >= 90 && n <= 97)) this.fg = true;
      else if (n === 39) this.fg = false;
      else if ((n >= 40 && n <= 47) || (n >= 100 && n <= 107)) this.bg = true;
      else if (n === 49) this.bg = false;
      else if (n === 38 || n === 48 || n === 58) {
        if (n === 38) this.fg = true;
        if (n === 48) this.bg = true;
        // 38;5;n and 38;2;r;g;b carry their color in the next parameters.
        if (colon < 0) i += parts[i + 1] === "5" ? 2 : parts[i + 1] === "2" ? 4 : 0;
      }
    }
  }
}

const enum St {
  Ground,
  Esc,
  EscInter,
  Csi,
  Str,
  StrEsc,
}

// Characters that end a piece of text. A tab doesn't: it only moves the
// cursor, so a line with tabs in it is matched as one.
const CONTROL = /[\x00-\x08\x0a-\x1f\x7f-\x9f]/g;

/** A piece of text cut off by the end of a batch waits this long for the
 *  rest, so a word split between two batches is still matched. Only after a
 *  big batch (a stream): what's typed and its echo come in small ones, and
 *  never wait. */
const HOLD_AFTER = 512;
const HOLD_MAX = 4096;
const HOLD_MS = 25;

/** The most time highlighting takes in any one second. Past that, in a
 *  flood (a `cat` of a big file), only the last TAIL_CHARS of each batch are
 *  highlighted until the second is up: the rest scrolls by too fast to read,
 *  and highlighting all of it would slow the window down (in macOS's and
 *  Linux's web views about three times), while the screen the flood ends on
 *  is still highlighted. What programs set is followed throughout. */
const BUDGET_MS = 300;
const TAIL_CHARS = 16 * 1024;

/** Plain text that may be highlighted, with the attributes it's printed in
 *  (a highlight's codes put back what they change). */
interface Plain {
  text: string;
  bold: boolean;
  dim: boolean;
  underline: boolean;
}

export class HighlightStream {
  private decoder = new TextDecoder();
  private st = St.Ground;
  private csi = "";
  private attrs = new Attrs();
  private alt = false;
  /** The end of the last batch, waiting for the rest of its line. */
  private held: Plain | null = null;
  private timer = 0;
  private matcher: Matcher | null = null;
  private budgetStart = 0;
  private spent = 0;

  /** `late`: text held back at the end of a batch, once it's done waiting. */
  constructor(private readonly late: (text: string) => void) {}

  get active(): boolean {
    return this.matcher !== null;
  }

  /** A new matcher (null: off). Turning it on mid-stream starts from what
   *  the terminal shows now: `onAltScreen`, and no program colors. Returns
   *  text held back by the old one, to write first. */
  setMatcher(m: Matcher | null, onAltScreen: boolean): string {
    const held = this.takeHeld();
    if (!this.matcher && m) {
      this.st = St.Ground;
      this.attrs.reset();
      this.alt = onAltScreen;
      this.decoder = new TextDecoder();
    }
    this.matcher = m;
    return held;
  }

  /** The shell was replaced (a reconnect): what the old one set is gone. */
  reset(): string {
    const held = this.takeHeld();
    this.st = St.Ground;
    this.attrs.reset();
    this.alt = false;
    this.decoder = new TextDecoder();
    return held;
  }

  dispose() {
    if (this.timer) window.clearTimeout(this.timer);
    this.timer = 0;
  }

  /** One batch of output, as the text to give xterm. */
  push(bytes: Uint8Array): string {
    if (this.timer) {
      window.clearTimeout(this.timer);
      this.timer = 0;
    }
    const t0 = performance.now();
    if (t0 - this.budgetStart > 1000) {
      this.budgetStart = t0;
      this.spent = 0;
    }
    const text = this.decoder.decode(bytes, { stream: true });
    const parts = this.parse(text);
    // A batch that ends partway through a line: after a big one (a stream),
    // the rest is probably on its way, so that part waits for it.
    const last = parts[parts.length - 1];
    if (last && typeof last !== "string" && text.length >= HOLD_AFTER && last.text.length < HOLD_MAX) {
      this.held = parts.pop() as Plain;
      this.timer = window.setTimeout(() => {
        this.timer = 0;
        const t = this.takeHeld();
        if (t) this.late(t);
      }, HOLD_MS);
    }
    const out = this.render(parts, this.spent > BUDGET_MS ? tailStart(parts) : 0);
    this.spent += performance.now() - t0;
    return out;
  }

  /** What's held back, highlighted, and nothing held anymore. */
  private takeHeld(): string {
    if (this.timer) {
      window.clearTimeout(this.timer);
      this.timer = 0;
    }
    const held = this.held;
    this.held = null;
    return held ? this.render([held]) : "";
  }

  /** The batch's pieces in order: text as it came, and plain text, all of
   *  which is matched in one go; pieces before `from` go out as they came. */
  private render(parts: (string | Plain)[], from = 0): string {
    let out = "";
    for (let i = 0; i < from; i++) {
      const p = parts[i];
      out += typeof p === "string" ? p : p.text;
    }
    const rest = from ? parts.slice(from) : parts;
    const plain = rest.filter((p): p is Plain => typeof p !== "string");
    if (!plain.length || !this.matcher) return out + rest.map((p) => (typeof p === "string" ? p : p.text)).join("");
    const spans = this.matcher.find(plain.map((p) => p.text).join("\n"));
    let k = 0;
    let base = 0;
    for (const p of rest) {
      if (typeof p === "string") {
        out += p;
        continue;
      }
      const end = base + p.text.length;
      let at = 0;
      while (k < spans.length && spans[k][0] < end) {
        const sp = spans[k++];
        out += p.text.slice(at, sp[0] - base) + wrap(p.text.slice(sp[0] - base, sp[1] - base), sp[2], p);
        at = sp[1] - base;
      }
      out += at ? p.text.slice(at) : p.text;
      base = end + 1;
    }
    return out;
  }

  private parse(text: string): (string | Plain)[] {
    const parts: (string | Plain)[] = [];
    const n = text.length;
    // Everything from `copy` on goes out as it came, up to the next piece of
    // plain text.
    let copy = 0;
    let i = 0;
    // What the last batch held back: the start of this batch's first line.
    let carry = this.held;
    this.held = null;
    while (i < n) {
      if (this.st === St.Ground) {
        CONTROL.lastIndex = i;
        const m = CONTROL.exec(text);
        const j = m ? m.index : n;
        if (j > i && this.matcher && !this.alt && this.attrs.plain) {
          if (i > copy) parts.push(text.slice(copy, i));
          const a = this.attrs;
          if (carry && i === 0) carry.text += text.slice(0, j);
          else parts.push({ text: text.slice(i, j), bold: a.bold, dim: a.dim, underline: a.underline });
          copy = j;
        }
        if (carry) {
          // Its line went on (it's first in this batch) or ended before.
          parts.splice(0, 0, carry);
          carry = null;
        }
        if (!m) break;
        const c = text.charCodeAt(j);
        i = j + 1;
        if (c === 0x1b) this.st = St.Esc;
        else if (c === 0x9b) this.enterCsi();
        else if (c === 0x9d || c === 0x90 || c === 0x98 || c === 0x9e || c === 0x9f) this.st = St.Str;
        continue;
      }
      const c = text.charCodeAt(i);
      i++;
      if (c === 0x18 || c === 0x1a) {
        this.st = St.Ground;
        continue;
      }
      switch (this.st) {
        case St.Esc:
          if (c === 0x5b) this.enterCsi();
          else if (c === 0x5d || c === 0x50 || c === 0x58 || c === 0x5e || c === 0x5f) this.st = St.Str;
          else if (c >= 0x20 && c <= 0x2f) this.st = St.EscInter;
          else if (c === 0x63) {
            // ESC c: a full reset.
            this.attrs.reset();
            this.alt = false;
            this.st = St.Ground;
          } else if (c >= 0x30 && c <= 0x7e) this.st = St.Ground;
          break;
        case St.EscInter:
          if (c === 0x1b) this.st = St.Esc;
          else if (c >= 0x30 && c <= 0x7e) this.st = St.Ground;
          break;
        case St.Csi:
          if (c === 0x1b) this.st = St.Esc;
          else if (c >= 0x20 && c <= 0x3f) {
            if (this.csi.length < 128) this.csi += text[i - 1];
          } else if (c >= 0x40 && c <= 0x7e) {
            this.endCsi(c);
            this.st = St.Ground;
          }
          break;
        case St.Str:
          if (c === 0x1b) this.st = St.StrEsc;
          else if (c === 0x07 || c === 0x9c) this.st = St.Ground;
          break;
        case St.StrEsc:
          if (c === 0x5c) this.st = St.Ground;
          else {
            // ESC ended the string and starts something else.
            this.st = St.Esc;
            i--;
          }
          break;
      }
    }
    if (carry) parts.splice(0, 0, carry);
    if (copy < n) parts.push(text.slice(copy));
    return parts;
  }

  private enterCsi() {
    this.st = St.Csi;
    this.csi = "";
  }

  private endCsi(final: number) {
    const p = this.csi;
    if (final === 0x6d) {
      // SGR, unless it's a private or intermediate form (`ESC [ > 4 ; 2 m`).
      if (!/[<=>?\x20-\x2f]/.test(p)) this.attrs.apply(p);
    } else if ((final === 0x68 || final === 0x6c) && p.startsWith("?")) {
      // The alternate screen: on with `ESC [ ? 1049 h`, off with `l`.
      if (p.slice(1).split(";").some((x) => x === "1049" || x === "1047" || x === "47")) this.alt = final === 0x68;
    } else if (final === 0x70 && p === "!") {
      // ESC [ ! p: a soft reset, attributes included.
      this.attrs.reset();
    }
  }
}

/** Where the last TAIL_CHARS of a batch's text start, as an index into its
 *  pieces. */
function tailStart(parts: (string | Plain)[]): number {
  let chars = 0;
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i];
    chars += typeof p === "string" ? p.length : p.text.length;
    if (chars > TAIL_CHARS) return i + 1;
  }
  return 0;
}

/** `text` in `style`, then back to `at`'s attributes. */
function wrap(text: string, style: HighlightStyle, at: Plain): string {
  let on = String(colorById.get(style.color)?.sgr ?? 39);
  let off = "39";
  if (style.bold && !at.bold) {
    on += ";1";
    off += at.dim ? ";22;2" : ";22";
  }
  if (style.underline && !at.underline) {
    on += ";4";
    off += ";24";
  }
  return `\x1b[${on}m${text}\x1b[${off}m`;
}

// ── Choices ─────────────────────────────────────────────────────────────────

export const HIGHLIGHT_KEY = "submarine-terminal-highlight";

/** This device's set (Settings › Terminal). */
export function readHighlightId(): string {
  try {
    const id = localStorage.getItem(HIGHLIGHT_KEY) || DEFAULT_HIGHLIGHT;
    return setById.has(id) ? id : DEFAULT_HIGHLIGHT;
  } catch {
    return DEFAULT_HIGHLIGHT;
  }
}

// Each saved server's own set and the user's rules, kept current by
// DesktopApp (from the server list, and get_highlight_rules); a terminal
// looks them up when settings change, like its color scheme.
const serverSets = new Map<number, string>();
let customRules: readonly CustomRule[] = [];
const matchers = new Map<string, Matcher | null>();

export function setServerHighlights(servers: { id: number; highlight?: string | null }[]): boolean {
  const next = new Map<number, string>();
  for (const s of servers) if (s.highlight && setById.has(s.highlight)) next.set(s.id, s.highlight);
  const same = next.size === serverSets.size && [...next].every(([k, v]) => serverSets.get(k) === v);
  if (same) return false;
  serverSets.clear();
  for (const [k, v] of next) serverSets.set(k, v);
  return true;
}

export function setCustomRules(rules: readonly CustomRule[]): boolean {
  if (JSON.stringify(rules) === JSON.stringify(customRules)) return false;
  customRules = rules;
  matchers.clear();
  return true;
}

export const readCustomRules = (): readonly CustomRule[] => customRules;

/** The set a terminal of this server uses: its own, else this device's. */
export function highlightIdForServer(serverId: number | undefined): string {
  return (serverId && serverSets.get(serverId)) || readHighlightId();
}

export function matcherForServer(serverId: number | undefined): Matcher | null {
  const id = highlightIdForServer(serverId);
  if (!matchers.has(id)) matchers.set(id, buildMatcher(id, customRules));
  return matchers.get(id)!;
}

// Made-up terminal output for timing (perf-io.mjs, tools/test/perf.test.ts):
// the same bytes every run, so numbers compare from one run to the next.
// Nothing here is a real server's output.

/** A small generator of the same numbers every time (mulberry32). */
const random = (seed) => () => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

const WORDS = "the quick brown fox jumps over lazy dog server request handled cache worker queue job user session token page index build deploy node disk memory load".split(" ");
const LEVELS = ["INFO", "INFO", "INFO", "DEBUG", "WARNING", "ERROR", "OK", "INFO"];
const pick = (r, list) => list[Math.floor(r() * list.length)];
const words = (r, n) => Array.from({ length: n }, () => pick(r, WORDS)).join(" ");

const kinds = {
  /** Plain text, as a `cat` of a text file. */
  plain: (r) => `${words(r, 8 + Math.floor(r() * 10))}\r\n`,
  /** An application log: dates, levels, addresses, a link now and then. */
  log: (r, i) => {
    const s = String(i % 60).padStart(2, "0");
    const tail = r() < 0.2 ? ` see https://wiki.example.com/runbook/${Math.floor(r() * 900)}` : r() < 0.3 ? " connection refused, retrying" : "";
    return `2026-11-07 10:${String(Math.floor(i / 60) % 60).padStart(2, "0")}:${s} ${pick(r, LEVELS).padEnd(7)} api   ${words(r, 5)} from 192.0.2.${Math.floor(r() * 250)}:${1024 + Math.floor(r() * 60000)} took ${Math.floor(r() * 900)} ms${tail}\r\n`;
  },
  /** Text a program colored itself, as a build's or a test run's output. */
  sgr: (r) => {
    let line = "";
    for (let k = 0; k < 6; k++) line += `\x1b[${r() < 0.3 ? "1;" : ""}${31 + Math.floor(r() * 6)}m${words(r, 2)}\x1b[0m `;
    return `${line}\r\n`;
  },
  /** `ls -l --color`: plain columns, then a colored name. */
  ls: (r, i) => {
    const color = pick(r, ["01;34", "01;32", "0", "01;36", "0", "0"]);
    return `-rw-r--r-- 1 deploy deploy ${String(Math.floor(r() * 1e7)).padStart(8)} Nov  7 10:${String(i % 60).padStart(2, "0")} \x1b[${color}m${pick(r, WORDS)}-${Math.floor(r() * 1e4)}.${pick(r, ["log", "conf", "tar.gz", "sh", "txt"])}\x1b[0m\r\n`;
  },
  /** `yes`. */
  yes: () => "y\r\n",
};

/** `bytes` of made-up output of one kind (plain, log, sgr, ls, yes), or
 *  "mixed": a few hundred lines of each in turn. Whole lines, so a little over. */
export const corpus = (kind, bytes, seed = 1) => {
  const r = random(seed);
  const order = kind === "mixed" ? ["plain", "log", "sgr", "ls"] : [kind];
  const out = [];
  let size = 0;
  for (let i = 0; size < bytes; i++) {
    const line = kinds[order[Math.floor(i / 300) % order.length]](r, i);
    out.push(line);
    size += line.length;
  }
  return new TextEncoder().encode(out.join(""));
};

/** One redraw of a `top`-like screen (cursor home, lines cleared and
 *  rewritten), `rows` lines of it. */
export const topFrame = (frame, rows = 40) => {
  const r = random(1000 + frame);
  let out = "\x1b[H\x1b[1mtop - 10:41:02 up 23 days,  4:11,  1 user,  load average: 0.21, 0.18, 0.12\x1b[0m\x1b[K\r\n";
  out += `Tasks: 212 total,   1 running, 211 sleeping\x1b[K\r\n\x1b[7m    PID USER      PR  NI    VIRT    RES    SHR S  %CPU  %MEM     TIME+ COMMAND\x1b[0m\x1b[K\r\n`;
  for (let i = 0; i < rows - 3; i++) {
    out += `${String(1000 + Math.floor(r() * 60000)).padStart(7)} deploy    20   0 ${String(Math.floor(r() * 1e6)).padStart(7)} ${String(Math.floor(r() * 1e5)).padStart(6)} ${String(Math.floor(r() * 1e4)).padStart(6)} S ${(r() * 100).toFixed(1).padStart(5)} ${(r() * 10).toFixed(1).padStart(5)} ${String(Math.floor(r() * 99)).padStart(4)}:${String(Math.floor(r() * 60)).padStart(2, "0")}.${String(Math.floor(r() * 99)).padStart(2, "0")} ${pick(r, WORDS)}\x1b[K\r\n`;
  }
  return new TextEncoder().encode(out);
};

/** `data` cut the way the backend hands it over: pieces of `size` (its
 *  FLUSH_CAP is 256 KiB). */
export const batches = (data, size = 256 * 1024) => {
  const out = [];
  for (let at = 0; at < data.length; at += size) out.push(data.subarray(at, at + size));
  return out;
};

// Times a terminal's output and input: the real page in Chromium,
// with mocks.js's backend, fed made-up output (corpus.mjs) the way the
// backend hands it over, and typed into.
//
//   node tools/screenshots/perf-io.mjs                 every measure, 5 runs each
//   node tools/screenshots/perf-io.mjs output input    some of them (output, stream, input, broadcast)
//   RUNS=3 MB=5 GPU=1 node tools/screenshots/perf-io.mjs
//   ROOT=/path/to/the/original/project node tools/screenshots/perf-io.mjs
//   KINDS=log,ls    only those kinds of output; PROFILE=1 says where the time went
//   STORAGE='{"key":"value"}'  more of the page's localStorage, to try a setting
//   JSON=out.json   also saves the medians
//   BASE=out.json   compares with an earlier run's JSON, and fails when the page
//                   is busier by more than TOLERANCE percent (25)
//   CHECK=1         fails when a count is over perf-io.budget.json's
//
// What it prints, each the median of the runs:
//   - output: a burst of MB megabytes of each kind, in the backend's 256 KiB
//     batches, no more on its way than the backend allows (4 MB not drawn).
//     "ms" is from the first batch to the page saying it drew the last
//     (within its 50 ms report), "busy ms" the page's main thread at work
//     meanwhile, "longest ms" its longest hold without a break, "frames" how
//     many times it drew.
//   - stream: `yes` and a `top`-like redraw, for three seconds each.
//   - input: a key to the write call, and to the echo drawn (the mock's shell
//     echoes at once, so this is the page's own part).
//   - broadcast: the same to 36 servers, and the calls across the bridge.
//
// The numbers are this machine's and headless Chromium's, with no GPU and a
// made-up backend: no Tauri bridge, no SSH, no Windows. Compare a run with
// another on the same machine. The counts (calls, writes) are the same
// everywhere.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import fs from "node:fs";
import path from "node:path";
import { createServer, mergeConfig } from "vite";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(process.env.ROOT ?? path.resolve(here, "../.."));
const RUNS = Number(process.env.RUNS ?? 5);
const MB = Number(process.env.MB ?? 5);
const SERVERS = Number(process.env.SERVERS ?? 36);
const wanted = process.argv.slice(2);
const want = (name) => wanted.length === 0 || wanted.includes(name);
process.chdir(root);

// The page of `root` (this project's, or the original's) with this folder's
// mocks loaded first.
const mocksFirst = {
  name: "perf-mocks-first",
  transformIndexHtml: (html) => html.replace(
    '<script type="module" src="/src/main.tsx"></script>',
    `<script type="module">import "/@fs${path.join(here, "mocks.js")}"; import "/src/main.tsx";</script>`,
  ),
};
const appConfig = (await import(path.join(root, "vite.config.ts"))).default;
const server = await createServer(mergeConfig(await appConfig({ command: "serve", mode: "development" }), {
  configFile: false,
  root,
  logLevel: "warn",
  plugins: [mocksFirst],
  server: { port: 1440, strictPort: false, host: "127.0.0.1", hmr: false, fs: { allow: [root, here] } },
}));
await server.listen();
const browser = await chromium.launch();
const CORPUS = `/@fs${path.join(here, "corpus.mjs")}`;

const median = (list) => {
  const s = [...list].sort((a, b) => a - b);
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};
/** The median of each number in `runs` (objects of the same shape), rounded. */
const medians = (runs) => Object.fromEntries(Object.keys(runs[0]).map((k) => {
  const m = median(runs.map((r) => r[k]));
  return [k, Math.abs(m) >= 10 ? Math.round(m) : Math.round(m * 100) / 100];
}));

const REGEX_RULES = [
  { id: 1, pattern: "took \\d+ ms", kind: "regex", matchCase: false, color: "yellow", bold: false, underline: false, enabled: true },
  { id: 2, pattern: "job-[a-z]+", kind: "regex", matchCase: false, color: "blue", bold: false, underline: false, enabled: true },
  { id: 3, pattern: "\\b[A-Z]{3}-\\d+\\b", kind: "regex", matchCase: true, color: "green", bold: true, underline: false, enabled: true },
];
/** The highlighting a run is taken with: none (as the original project
 *  has), the standard set (the default), and that plus three rules of the
 *  user's own, each a regular expression. */
const HIGHLIGHTS = { off: { set: "none" }, standard: { set: "standard" }, "standard+3 regex": { set: "standard", rules: REGEX_RULES } };

/** A page with `servers` more servers than the three that open by
 *  themselves, and web1's terminal in front; `storage` goes into
 *  localStorage first. */
const open = async (storage = {}, servers = 0) => {
  const context = await browser.newContext({ viewport: { width: 1720, height: 960 }, locale: "en-GB", timezoneId: "UTC" });
  await context.addInitScript(({ storage, servers }) => {
    localStorage.setItem("mock-servers", String(servers));
    localStorage.setItem("mock-echo-ms", "now");
    localStorage.setItem("submarine-session-tree-open", "0");
    localStorage.setItem("submarine-ai-pane-folded", "1");
    for (const [k, v] of Object.entries(storage)) localStorage.setItem(k, v);
    window.__long = [];
    new PerformanceObserver((list) => { for (const e of list.getEntries()) window.__long.push(e.duration); }).observe({ type: "longtask", buffered: true });
    // When each key went down, before the page's own handlers see it.
    window.__keys = [];
    window.addEventListener("keydown", () => window.__keys.push(performance.now()), true);
  }, { storage: { "submarine-terminal-gpu-v2": process.env.GPU ? "on" : "off", ...JSON.parse(process.env.STORAGE ?? "{}"), ...storage }, servers });
  const page = await context.newPage();
  page.on("pageerror", (e) => console.warn(`page error: ${e.message}`));
  await page.goto(server.resolvedUrls.local[0]);
  const last = servers ? `node${String(servers).padStart(2, "0")}` : "db1";
  await page.getByRole("button", { name: last }).first().waitFor({ timeout: 90000 });
  await page.waitForFunction((n) => window.__perf.sessions.size >= n, servers + 3, { timeout: 90000 });
  await page.getByRole("button", { name: "web1" }).first().click();
  await page.waitForTimeout(servers ? 3000 : 1200);
  const id = await page.evaluate(() => [...window.__perf.sessions].find(([, s]) => /^session-1($|::)/.test(s))?.[0]);
  if (!id) throw new Error("web1's terminal didn't open");
  const cdp = await context.newCDPSession(page);
  await cdp.send("Performance.enable");
  /** The main thread's time at work so far, in ms. */
  const busy = async (name = "TaskDuration") => {
    const { metrics } = await cdp.send("Performance.getMetrics");
    return 1000 * metrics.find((m) => m.name === name).value;
  };
  /** Where the page's time goes while `run` runs: the functions it was in most (PROFILE=1). */
  const profiled = async (run) => {
    await cdp.send("Profiler.enable");
    await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
    await cdp.send("Profiler.start");
    const out = await run();
    const { profile } = await cdp.send("Profiler.stop");
    const self = new Map();
    const byId = new Map(profile.nodes.map((n) => [n.id, n]));
    profile.samples.forEach((nodeId, i) => {
      const n = byId.get(nodeId);
      const k = `${n.callFrame.functionName || "(anon)"} ${n.callFrame.url.split("/").slice(-2).join("/").split("?")[0]}:${n.callFrame.lineNumber}`;
      self.set(k, (self.get(k) ?? 0) + (profile.timeDeltas[i] ?? 0) / 1000);
    });
    console.log([...self].filter(([k]) => !k.startsWith("(idle)")).sort((a, b) => b[1] - a[1]).slice(0, 30).map(([k, v]) => `${v.toFixed(1).padStart(8)} ms  ${k}`).join("\n"));
    return out;
  };
  return { context, page, id, busy, profiled };
};

const highlightStorage = (h) => ({
  "submarine-terminal-highlight": HIGHLIGHTS[h].set,
  "mock-highlight-rules": JSON.stringify(HIGHLIGHTS[h].rules ?? []),
});

/** In the page: hand `id` its output as the backend does, and say how it
 *  went (window.__feed, made here and run by the caller). `feed` is { kind, bytes } for a burst, or { stream, ms } for `yes`
 *  (4 KiB every 8 ms, the backend's flush window) or `top` (a redraw every
 *  50 ms). */
const feedInPage = async ({ id, corpusUrl, feed }) => {
  const { corpus, batches, topFrame } = await import(corpusUrl);
  const perf = window.__perf;
  const turn = () => new Promise((done) => { const c = new MessageChannel(); c.port1.onmessage = () => done(); c.port2.postMessage(0); });
  const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
  const drawn = () => perf.drawn.get(id) ?? 0;
  // Made ahead of the clock: the backend's work, not the page's.
  const ready = feed.kind
    ? batches(corpus(feed.kind, feed.bytes)).map((b) => [perf.prepare(id, b), b.length])
    : Array.from({ length: feed.stream === "yes" ? 1 : 64 }, (_, i) => {
      const b = feed.stream === "yes" ? new TextEncoder().encode("y\r\n".repeat(1365)) : topFrame(i);
      return [perf.prepare(id, b), b.length];
    });
  window.__feed = async () => {
    const before = drawn();
    let frames = 0;
    let gap = 0;
    let counting = true;
    window.__long.length = 0;
    let handed = 0;
    const t0 = performance.now();
    let lastFrame = t0;
    const frame = () => {
      if (!counting) return;
      const now = performance.now();
      frames++;
      gap = Math.max(gap, now - lastFrame);
      lastFrame = now;
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
    if (feed.kind) {
      for (const [prepared, n] of ready) {
        // No more on its way than the backend allows (IN_FLIGHT_MAX).
        while (handed - (drawn() - before) >= 4 << 20) await sleep(2);
        perf.deliver(id, prepared);
        handed += n;
        await turn();
      }
    } else {
      for (let i = 0; performance.now() - t0 < feed.ms; i++) {
        const [prepared, n] = ready[i % ready.length];
        perf.deliver(id, typeof prepared === "string" ? prepared : prepared.slice(0));
        handed += n;
        await sleep(feed.stream === "yes" ? 8 : 50);
      }
    }
    while (drawn() - before < handed && performance.now() - t0 < 120000) await sleep(2);
    const ms = performance.now() - t0;
    counting = false;
    await sleep(60);
    const long = window.__long.splice(0);
    return { ms, frames, gap, bytes: handed, drawn: drawn() - before, long: long.reduce((a, b) => a + b, 0), longest: Math.max(0, ...long) };
  };
};

const output = async (highlight, feed) => {
  const runs = [];
  for (let run = 0; run < RUNS; run++) {
    const { context, page, id, busy, profiled } = await open(highlightStorage(highlight));
    await page.evaluate(feedInPage, { id, corpusUrl: CORPUS, feed });
    await page.waitForTimeout(200);
    const before = await busy();
    // PARTS=1: the busy time by what it went on (script, layout, styles).
    const parts = process.env.PARTS ? ["ScriptDuration", "LayoutDuration", "RecalcStyleDuration"] : [];
    const partsBefore = await Promise.all(parts.map((n) => busy(n)));
    // TIMERS=1: who set timers while the output came, and how many.
    if (process.env.TIMERS) await page.evaluate(() => {
      window.__timers = new Map();
      const set = window.setTimeout;
      window.setTimeout = function (...a) {
        const who = (new Error().stack ?? "").split("\n").slice(2, 5).map((l) => l.trim().replace(/https?:\/\/[^/]+\//, "").replace(/\?[^:]*/, "")).join(" < ");
        window.__timers.set(who, (window.__timers.get(who) ?? 0) + 1);
        return set.apply(this, a);
      };
    });
    const feedNow = () => page.evaluate(() => window.__feed());
    const r = process.env.PROFILE && run === 0 ? await profiled(feedNow) : await feedNow();
    if (process.env.TIMERS && run === 0) console.log((await page.evaluate(() => [...window.__timers].sort((a, b) => b[1] - a[1]).slice(0, 8))).map(([who, n]) => `${String(n).padStart(7)} timers  ${who}`).join("\n"));
    if (r.drawn < r.bytes) console.warn(`${highlight} ${JSON.stringify(feed)}: only ${r.drawn} of ${r.bytes} bytes drawn`);
    runs.push({
      ms: r.ms, "busy ms": (await busy()) - before, "longest ms": r.longest, "long tasks ms": r.long,
      frames: r.frames, "longest gap ms": r.gap, "MB/s": r.bytes / 1048576 / (r.ms / 1000),
      ...Object.fromEntries(await Promise.all(parts.map(async (n, i) => [n.replace("Duration", " ms"), (await busy(n)) - partsBefore[i]]))),
    });
    await context.close();
  }
  return medians(runs);
};

/** Types `text` a key at a time, and says for each key how long until the
 *  first write went to the backend, how many calls it took, and how long
 *  until the terminal's rows changed (the echo drawn; the DOM renderer's). */
const typeAndTime = async (page, text, { watchRows = true, click = true } = {}) => {
  if (click) {
    // The first terminal on screen (the original project's page keeps others laid out past its edge).
    const at = await page.evaluate(() => {
      for (const el of document.querySelectorAll(".xterm-screen")) {
        const r = el.getBoundingClientRect();
        if (r.width > 40 && r.height > 40 && r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight) return [r.left + r.width / 2, r.top + r.height / 2];
      }
      return null;
    });
    if (!at) throw new Error("no terminal on screen to type into");
    await page.mouse.click(at[0], at[1]);
  }
  await page.waitForTimeout(300);
  await page.evaluate((watchRows) => {
    window.__keys.length = 0;
    window.__perf.writes.length = 0;
    window.__rows = [];
    if (!watchRows) return;
    const rows = [...document.querySelectorAll(".xterm-rows")].find((r) => r.getClientRects().length);
    if (rows) new MutationObserver(() => window.__rows.push(performance.now())).observe(rows, { childList: true, subtree: true, characterData: true });
  }, watchRows);
  await page.keyboard.type(text, { delay: 40 });
  await page.waitForTimeout(400);
  return page.evaluate(() => {
    const keys = window.__keys;
    const writes = window.__perf.writes;
    const out = [];
    keys.forEach((at, i) => {
      const until = keys[i + 1] ?? Infinity;
      const mine = writes.filter((w) => w.at >= at && w.at < until);
      const drawn = window.__rows.find((t) => t >= at && t < until);
      if (!mine.length) return;
      out.push({
        first: mine[0].at - at,
        last: mine[mine.length - 1].at - at,
        calls: mine.length,
        targets: mine.reduce((a, w) => a + w.targets, 0),
        drawn: drawn === undefined ? NaN : drawn - at,
      });
    });
    return out;
  });
};

const keyStats = (perKey) => ({
  "key to first write ms": median(perKey.map((k) => k.first)),
  "key to last write ms": median(perKey.map((k) => k.last)),
  "worst key to last write ms": Math.max(...perKey.map((k) => k.last)),
  "calls per key": median(perKey.map((k) => k.calls)),
  "writes per key": median(perKey.map((k) => k.targets)),
  "key to echo drawn ms": median(perKey.map((k) => k.drawn).filter((d) => !Number.isNaN(d))),
});

const TEXT = "echo hello from submarine on every server";

const input = async (highlight) => {
  const runs = [];
  for (let run = 0; run < RUNS; run++) {
    const { context, page } = await open(highlightStorage(highlight));
    const typed = keyStats(await typeAndTime(page, TEXT));
    // The first key after the terminal was left alone: its cursor has stopped
    // blinking by then (10 s), and the key starts it again.
    await page.waitForTimeout(10500);
    const [first] = await typeAndTime(page, "x", { click: false });
    runs.push({ ...typed, "after 10 s idle: key to echo drawn ms": first?.drawn ?? NaN });
    await context.close();
  }
  return medians(runs);
};

/** Broadcast to every server, as perf.mjs sets it up: each pinned to the
 *  Wall, Broadcast on for all, typed into the first tile. */
const broadcast = async () => {
  const runs = [];
  for (let run = 0; run < Math.min(RUNS, 3); run++) {
    const { context, page, busy } = await open(highlightStorage("standard"), SERVERS);
    const tab = (name) => page.getByRole("button", { name }).first();
    for (let i = 1; i <= SERVERS; i++) {
      await tab(`node${String(i).padStart(2, "0")}`).click({ button: "right" });
      await page.getByText("Pin active terminal to Wall").click();
    }
    await page.getByRole("button", { name: "Wall pinboard" }).click();
    await page.waitForTimeout(2500);
    await page.getByRole("button", { name: "Broadcast input" }).click();
    await page.getByRole("checkbox").first().check();
    await page.getByRole("button", { name: "All", exact: true }).click();
    await page.mouse.click(860, 940);
    await page.waitForTimeout(1000);
    const before = await busy();
    const perKey = await typeAndTime(page, TEXT, { watchRows: false });
    runs.push({ ...keyStats(perKey), "busy ms per key": ((await busy()) - before) / TEXT.length });
    await context.close();
  }
  const m = medians(runs);
  delete m["key to echo drawn ms"];
  return m;
};

const results = { root, runs: RUNS, mb: MB, gpu: !!process.env.GPU, output: {}, stream: {}, input: {}, broadcast: null };
let failed = false;
try {
  if (want("output")) {
    const rows = {};
    for (const kind of (process.env.KINDS ?? "plain,log,sgr,ls,mixed").split(",")) {
      for (const h of Object.keys(HIGHLIGHTS)) {
        if ((process.env.ROOT || process.env.OFF) && h !== "off") continue; // the original project has no highlighting
        if (process.env.ONLY && h !== process.env.ONLY) continue;
        rows[`${kind}, highlighting ${h}`] = results.output[`${kind}/${h}`] = await output(h, { kind, bytes: MB << 20 });
      }
    }
    console.log(`\nA burst of ${MB} MB (${RUNS} runs, medians)`);
    console.table(rows);
  }
  if (want("stream")) {
    const rows = {};
    for (const stream of ["yes", "top"]) {
      for (const h of ["off", "standard"]) {
        if (process.env.ROOT && h !== "off") continue;
        rows[`${stream} for 3 s, highlighting ${h}`] = results.stream[`${stream}/${h}`] = await output(h, { stream, ms: 3000 });
      }
    }
    console.log(`\nA steady stream (${RUNS} runs, medians)`);
    console.table(rows);
  }
  if (want("input")) {
    const rows = {};
    for (const h of ["off", "standard"]) {
      if (process.env.ROOT && h !== "off") continue;
      rows[`one terminal, highlighting ${h}`] = results.input[h] = await input(h);
    }
    console.log(`\nTyping ${TEXT.length} keys into one terminal (${RUNS} runs, medians)`);
    console.table(rows);
  }
  if (want("broadcast")) {
    results.broadcast = await broadcast();
    console.log(`\nTyping ${TEXT.length} keys with Broadcast to ${SERVERS} servers (medians)`);
    console.table({ [`Broadcast to ${SERVERS}`]: results.broadcast });
  }
  if (process.env.JSON) fs.writeFileSync(path.resolve(here, "../..", process.env.JSON), `${JSON.stringify(results, null, 2)}\n`);
  if (process.env.BASE) {
    // Against an earlier run on this machine (its JSON=): how much busier
    // the page's main thread is for the same output. Over the tolerance
    // fails; a quarter by default, which is past what one run differs from
    // the next here (about a tenth).
    const base = JSON.parse(fs.readFileSync(path.resolve(here, "../..", process.env.BASE), "utf8"));
    const most = Number(process.env.TOLERANCE ?? 25);
    const rows = {};
    for (const part of ["output", "stream"]) {
      for (const [name, now] of Object.entries(results[part])) {
        const was = base[part]?.[name];
        if (!was) continue;
        const change = Math.round(100 * (now["busy ms"] / was["busy ms"] - 1));
        rows[`${part} ${name}`] = { "busy ms before": was["busy ms"], "busy ms now": now["busy ms"], "change %": change, "": change > most ? "SLOWER" : "" };
        if (change > most) failed = true;
      }
    }
    console.log(`\nAgainst ${process.env.BASE} (fails past +${most}%)`);
    console.table(rows);
  }
  if (process.env.CHECK) {
    // Counts only: they are the same on every machine, so they can't flake.
    const budget = JSON.parse(fs.readFileSync(path.join(here, "perf-io.budget.json"), "utf8"));
    const check = (what, got, most) => {
      if (got === undefined || got === null) return;
      const ok = got <= most;
      console.log(`${ok ? "ok  " : "OVER"}  ${what}: ${got} (at most ${most})`);
      if (!ok) failed = true;
    };
    for (const h of Object.keys(results.input)) check(`one terminal, highlighting ${h}: calls per key`, results.input[h]["calls per key"], budget["calls per key, one terminal"]);
    if (results.broadcast) {
      check(`Broadcast to ${SERVERS}: calls per key`, results.broadcast["calls per key"], budget["calls per key, broadcast"]);
      // The servers asked for, and the three that open by themselves.
      check(`Broadcast to ${SERVERS}: writes per key`, results.broadcast["writes per key"], SERVERS + 3);
    }
  }
} catch (e) {
  console.error(e.stack ?? e.message);
  failed = true;
} finally {
  await browser.close();
  await server.close();
}
process.exit(failed ? 1 : 0);

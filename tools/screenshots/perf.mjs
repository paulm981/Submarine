// Times what holds the window when many terminals change at once (#127):
// the real page in Chromium, with mocks.js's servers plus N more that open
// by themselves. Prints, for each step, how long the page was busy without
// a break (long tasks, 50 ms or more) and the longest single hold.
//
//   node tools/screenshots/perf.mjs [servers=36]
//
// The numbers are this machine's and headless Chromium's, not a user's
// Windows webview: compare a run before a change with one after, on the
// same machine, rather than reading them as the app's own.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createServer } from "vite";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const here = path.dirname(fileURLToPath(import.meta.url));
process.chdir(path.resolve(here, "../.."));
const count = Number(process.argv[2] ?? 36);

const server = await createServer({ configFile: path.join(here, "vite.config.mjs"), root: process.cwd(), logLevel: "warn" });
await server.listen();
const browser = await chromium.launch();

const settle = (page, ms = 1500) => page.waitForTimeout(ms);

/** What the page did since the last call: long tasks, as [total, longest, how many]. */
const took = (page) => page.evaluate(() => {
  const l = window.__long.splice(0);
  return [Math.round(l.reduce((a, b) => a + b, 0)), Math.round(Math.max(0, ...l)), l.length];
});

const steps = [];
const step = async (page, name, run) => {
  await took(page);
  const t0 = Date.now();
  await run();
  await settle(page);
  const [busy, longest, n] = await took(page);
  steps.push({ step: name, "busy ms": busy, "longest ms": longest, "long tasks": n, "wall ms": Date.now() - t0 });
};

try {
  const context = await browser.newContext({ viewport: { width: 1720, height: 960 }, locale: "en-GB", timezoneId: "UTC" });
  await context.addInitScript((n) => {
    localStorage.setItem("mock-servers", String(n));
    localStorage.setItem("submarine-session-tree-open", "0");
    localStorage.setItem("submarine-ai-pane-folded", "1");
    window.__long = [];
    new PerformanceObserver((list) => { for (const e of list.getEntries()) window.__long.push(e.duration); }).observe({ type: "longtask", buffered: true });
  }, count);
  const page = await context.newPage();
  // To try a style before it's in the app: PERF_CSS="…".
  if (process.env.PERF_CSS) await context.addInitScript((css) => {
    document.addEventListener("DOMContentLoaded", () => { const st = document.createElement("style"); st.textContent = css; document.head.append(st); });
  }, process.env.PERF_CSS);
  page.on("pageerror", (e) => console.warn(`page error: ${process.env.STACKS ? e.stack : e.message}`));
  const t0 = Date.now();
  await page.goto(server.resolvedUrls.local[0]);
  const last = `node${String(count).padStart(2, "0")}`;
  await page.getByRole("button", { name: last }).first().waitFor({ timeout: 60000 });
  await settle(page, 3000);
  const [busy, longest, n] = await took(page);
  steps.push({ step: `open ${count + 3} tabs`, "busy ms": busy, "longest ms": longest, "long tasks": n, "wall ms": Date.now() - t0 });

  const tab = (name) => page.getByRole("button", { name }).first();
  await step(page, "switch through 10 tabs", async () => {
    for (let i = 1; i <= 10; i++) await tab(`node${String(i).padStart(2, "0")}`).click();
  });
  if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT.replace(/\.png$/, "-tab.png") });

  // Every tab's terminal on the Wall.
  await step(page, `pin ${count} to the Wall`, async () => {
    for (let i = 1; i <= count; i++) {
      await tab(`node${String(i).padStart(2, "0")}`).click({ button: "right" });
      await page.getByText("Pin active terminal to Wall").click();
    }
  });
  const wall = page.getByRole("button", { name: "Wall pinboard" });
  await step(page, "Wall: open", () => wall.click());
  // Every tile opened in the end, not only the window kept answering.
  const opened = await page.evaluate(() => document.querySelectorAll(".xterm").length);
  console.log(`terminals open with the Wall shown: ${opened} (${count + 3} tabs, ${count} tiles)`);
  if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT });
  await step(page, "Wall: back to a tab", () => tab("node01").click());
  if (process.env.PROFILE) {
    const cdp = await context.newCDPSession(page);
    await cdp.send("Profiler.enable");
    await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
    await cdp.send("Profiler.start");
    await wall.click();
    await settle(page);
    const { profile } = await cdp.send("Profiler.stop");
    const self = new Map();
    const dt = profile.timeDeltas;
    const byId = new Map(profile.nodes.map((n) => [n.id, n]));
    profile.samples.forEach((id, i) => {
      const n = byId.get(id);
      const k = `${n.callFrame.functionName || "(anon)"} ${n.callFrame.url.split("/").slice(-2).join("/")}:${n.callFrame.lineNumber}`;
      self.set(k, (self.get(k) ?? 0) + (dt[i] ?? 0) / 1000);
    });
    console.log([...self].filter(([k]) => !k.startsWith("(idle)") && !k.startsWith("(program)")).sort((a, b) => b[1] - a[1]).slice(0, 25).map(([k, v]) => `${v.toFixed(1).padStart(7)} ms  ${k}`).join("\n"));
    await tab("node01").click();
    await settle(page);
  }
  await step(page, "Wall: open again", () => wall.click());
  await step(page, "Broadcast on, all", async () => {
    await page.getByRole("button", { name: "Broadcast input" }).click();
    await page.getByRole("checkbox").first().check();
    await page.getByRole("button", { name: "All", exact: true }).click();
    // The menu closes with a click outside it.
    await page.mouse.click(860, 940);
  });
  await step(page, "type 40 keys, Broadcast to all", async () => {
    await page.locator(".xterm-screen:visible").first().click();
    await page.keyboard.type("echo hello from submarine on every server", { delay: 15 });
  });
  console.log(`writes to servers while typing: ${await page.evaluate(() => window.__writes ?? 0)}`);
  await step(page, "Wall: to a tab and back x3", async () => {
    for (let i = 0; i < 3; i++) { await tab("node02").click(); await wall.click(); }
  });
  await step(page, "close all tabs", async () => {
    await tab("node01").click({ button: "right" });
    await page.getByText("Close all tabs").click();
    const sure = page.getByRole("button", { name: /^Close/ }).last();
    if (await sure.isVisible().catch(() => false)) await sure.click();
  });
} catch (e) {
  console.error(e.message.split("\n")[0]);
} finally {
  console.table(steps);
  await browser.close();
  await server.close();
}

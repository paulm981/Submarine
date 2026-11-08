// Takes the fork's screenshots in docs/screenshots/fork/ from the real page,
// run in a browser with the backend answered by mocks.js. See README.md.
//
//   node tools/screenshots/take.mjs [name ...]
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createServer } from "vite";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../..");
const out = path.join(repo, "docs/screenshots/fork");
process.chdir(repo);

const server = await createServer({ configFile: path.join(here, "vite.config.mjs"), root: repo, logLevel: "warn" });
await server.listen();
const url = server.resolvedUrls.local[0];
const browser = await chromium.launch();

// A fresh window, unlocked, with its server tabs connected. `store`: what
// this device has kept (localStorage) before the app starts.
const defaults = {
  "submarine-session-tree-open": "0",
  "submarine-session-tree-expanded-demo": "[1,2]",
  "submarine-ai-pane-folded": "1",
};
async function open(store = {}, size = { width: 1400, height: 900 }) {
  store = { ...defaults, ...store };
  const context = await browser.newContext({ viewport: size, deviceScaleFactor: 2, locale: "en-GB", timezoneId: "UTC" });
  await context.addInitScript((s) => { for (const [k, v] of Object.entries(s)) localStorage.setItem(k, v); }, store);
  const page = await context.newPage();
  page.on("pageerror", (e) => console.warn(`  page error: ${e.message}`));
  await page.goto(url);
  await page.getByRole("button", { name: "web1" }).first().waitFor();
  await page.waitForTimeout(1500);
  return page;
}

const save = async (page, name, clip) => {
  const file = path.join(out, `${name}.png`);
  await page.waitForTimeout(400);
  await page.screenshot({ path: file, clip, animations: "disabled" });
  const missing = await page.evaluate(() => [...window.__unhandled].sort());
  console.log(`${name}.png${missing.length ? `  (unanswered: ${missing.join(", ")})` : ""}`);
  await page.context().close();
};

// The box around an element, with some room.
const around = async (locator, pad = 16) => {
  const b = await locator.boundingBox();
  return { x: Math.max(0, b.x - pad), y: Math.max(0, b.y - pad), width: b.width + 2 * pad, height: b.height + 2 * pad };
};

const treeServer = (page, name) => page.locator("aside, nav, div").getByText(name, { exact: true }).last();

const shots = {
  // Settings › AI tools, with the tab row.
  async "settings-tabs"() {
    const page = await open({}, { width: 1400, height: 1000 });
    await page.getByTitle("Settings").first().click();
    await page.getByRole("tab", { name: "AI tools" }).click();
    await page.getByText("Folders the AI may copy into").waitFor();
    await save(page, "settings-tabs", { x: 0, y: 0, width: 980, height: 1000 });
  },

  // The AI panel: one command on three servers, a copy, a follow running.
  async "ai-card-group"() {
    const page = await open();
    await page.getByRole("button", { name: "AI panel" }).click();
    await save(page, "ai-card-group");
  },

  // Push files to servers, at its summary.
  async "bulk-push"() {
    const page = await open();
    await page.getByRole("button", { name: "web1" }).first().click({ button: "right" });
    await page.getByText("Push files to servers…").click();
    const dialog = page.getByRole("dialog", { name: "Push files to servers" });
    await dialog.getByRole("button", { name: "All" }).click();
    await dialog.getByRole("button", { name: /Choose files/ }).click();
    await dialog.locator("input.font-mono").fill("~/releases");
    await dialog.getByRole("button", { name: "Next" }).click();
    await dialog.getByRole("button", { name: "Start" }).waitFor();
    await save(page, "bulk-push", await around(dialog));
  },

  // A server's kept sessions (the session tree's Sessions…).
  async "kept-sessions"() {
    const page = await open({ "submarine-session-tree-open": "1" });
    await treeServer(page, "web1").click({ button: "right" });
    await page.getByRole("button", { name: "Sessions…" }).click();
    const dialog = page.getByRole("dialog");
    await dialog.getByText("Session 1").waitFor();
    await save(page, "kept-sessions", await around(dialog, 24));
  },

  // Settings › Servers & sessions: Keep sessions between connections.
  async "keep-sessions-setting"() {
    const page = await open();
    await page.getByTitle("Settings").first().click();
    await page.getByRole("tab", { name: "Servers & sessions" }).click();
    await save(page, "keep-sessions-setting", { x: 0, y: 0, width: 1400, height: 700 });
  },

  // A server's AI approval: the three levels.
  async "ai-levels"() {
    const page = await open({ "submarine-session-tree-open": "1" }, { width: 1400, height: 1200 });
    await treeServer(page, "web1").click({ button: "right" });
    await page.getByRole("button", { name: "AI approval…" }).click();
    const dialog = page.getByRole("dialog", { name: "AI approval: web1" });
    await save(page, "ai-levels", await around(dialog, 24));
  },

  // Broadcast, as the original project has it: typing goes to every tab ticked.
  async broadcast() {
    const page = await open();
    await page.getByRole("button", { name: "Broadcast input" }).click();
    await page.getByRole("checkbox").first().check();
    await page.getByRole("button", { name: "All", exact: true }).click();
    await save(page, "broadcast", { x: 0, y: 0, width: 1400, height: 240 });
  },
};

const wanted = process.argv.slice(2);
try {
  for (const [name, take] of Object.entries(shots)) {
    if (wanted.length && !wanted.includes(name)) continue;
    try { await take(); } catch (e) { console.error(`${name}: ${e.message.split("\n")[0]}`); }
  }
} finally {
  await browser.close();
  await server.close();
}

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

// The box around two elements and what's between them, with some room.
const spanning = async (from, to, pad = 16) => {
  const a = await from.boundingBox();
  const b = await to.boundingBox();
  const x = Math.min(a.x, b.x) - pad, y = Math.min(a.y, b.y) - pad;
  return { x: Math.max(0, x), y: Math.max(0, y), width: Math.max(a.x + a.width, b.x + b.width) - x + pad, height: Math.max(a.y + a.height, b.y + b.height) - y + pad };
};

// The settings card a heading or text is in.
const cardOf = (locator) => locator.locator("xpath=ancestor::div[contains(@class, 'rounded-2xl')][1]");

Object.assign(shots, {
  // The session tree beside a terminal whose output is highlighted.
  async "highlighting-and-tree"() {
    const page = await open({ "submarine-session-tree-open": "1", "submarine-session-tree-expanded-demo": "[1,2]" });
    await page.getByRole("button", { name: "web2" }).first().click();
    await page.getByText("CRITICAL").first().waitFor();
    await save(page, "highlighting-and-tree");
  },

  // Home's shell menu: the login shell as the default, and the others.
  async "home-shells"() {
    const page = await open();
    await page.getByRole("button", { name: "Home" }).first().click();
    await page.getByTitle("Choose a shell").first().click();
    await page.getByText("your login shell").waitFor();
    await save(page, "home-shells", { x: 0, y: 0, width: 800, height: 420 });
  },

  // Settings › Terminal: the color schemes and their preview.
  async "color-schemes"() {
    const page = await open({}, { width: 1400, height: 1700 });
    await page.getByTitle("Settings").first().click();
    await page.getByRole("tab", { name: "Terminal" }).click();
    await save(page, "color-schemes", await spanning(page.getByText("Colors", { exact: true }), page.getByRole("button", { name: "Use my own scheme…" })));
  },

  // Settings › Terminal: syntax highlighting.
  async "highlight-settings"() {
    const page = await open({}, { width: 1400, height: 2600 });
    await page.getByTitle("Settings").first().click();
    await page.getByRole("tab", { name: "Terminal" }).click();
    const card = cardOf(page.getByText("Highlighting", { exact: true }).first());
    await save(page, "highlight-settings", await around(card, 8));
  },

  // Settings › Help: Troubleshooting.
  async troubleshooting() {
    const page = await open();
    await page.getByTitle("Settings").first().click();
    await page.getByRole("tab", { name: "Help" }).click();
    const heading = page.getByText("Troubleshooting", { exact: true });
    await save(page, "troubleshooting", await spanning(heading, cardOf(page.getByRole("button", { name: /Copy diagnostics/i })), 12));
  },

  // The Profile panel, syncing through a git repository of your own.
  async "own-storage-sync"() {
    const page = await open({}, { width: 1000, height: 1200 });
    await page.getByTitle(/^Profile/).first().click();
    const sync = page.getByRole("button", { name: /Sync now/ });
    await sync.click();
    await page.getByText(/Last synced/).waitFor();
    await save(page, "own-storage-sync", await spanning(page.getByText("git@git.example.com:team/submarine-profiles.git").first(), page.getByText("Computers syncing this profile").locator("xpath=..")));
  },
});

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

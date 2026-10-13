// Third-party notices, generated before every build (npm run build) into
// public/licenses/third-party.json, which ships with the app and which
// Settings › Open-source licenses shows.
//
// What it lists is what ships: the JavaScript packages the app depends on at
// run time (package-lock.json entries that aren't dev-only) and the Rust
// crates compiled into it — the normal dependencies reachable from the app's
// crate on any platform, leaving out build scripts, tests and proc-macros,
// which only run while building. Each comes with the licence texts its own
// package carries (LICENSE*, COPYING*, NOTICE*); a package that carries none
// gets the standard text of its licence.
//
// Fonts are npm packages (@fontsource/*) and appear with them. The terminal
// color schemes aren't packages: Settings lists those from
// src/ui/terminalThemes.ts and their texts from public/licenses/color-schemes/.
//
// Needs cargo on the PATH (as building the app does). Without it, outside CI,
// it writes the JavaScript half and says so; in CI it fails the build rather
// than ship incomplete notices.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "public", "licenses", "third-party.json");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense|copyright)/i;

const texts = new Map(); // hash → text
const addText = (raw) => {
  const text = raw.replace(/\r\n?/g, "\n").replace(/[ \t]+$/gm, "").trim();
  if (!text) return null;
  const id = createHash("sha256").update(text).digest("hex").slice(0, 16);
  texts.set(id, text);
  return id;
};

const licenseFilesIn = (dir, extra) => {
  let names = [];
  try {
    names = readdirSync(dir).filter((f) => LICENSE_FILE.test(f) && statSync(join(dir, f)).isFile());
  } catch {
    return [];
  }
  if (extra && existsSync(join(dir, extra)) && !names.includes(extra)) names.push(extra);
  return names.sort().map((f) => readFileSync(join(dir, f), "utf8"));
};

const repoUrl = (r) => {
  const u = typeof r === "string" ? r : r?.url;
  if (!u) return null;
  return u.replace(/#.*$/, "").replace(/^git\+/, "").replace(/\.git$/, "").replace(/^git:\/\//, "https://").replace(/^github:/, "https://github.com/");
};

// ── JavaScript packages ─────────────────────────────────────────────────────

function npmPackages() {
  const lock = JSON.parse(readFileSync(join(ROOT, "package-lock.json"), "utf8"));
  const out = [];
  // Dev-only, but its generated CSS (with its preflight reset) is in the app.
  const shippedDev = new Set(["node_modules/tailwindcss"]);
  for (const [path, entry] of Object.entries(lock.packages || {})) {
    if (!path.startsWith("node_modules/")) continue;
    if (entry.dev && !shippedDev.has(path)) continue;
    const dir = join(ROOT, path);
    if (!existsSync(join(dir, "package.json"))) continue; // an optional one not installed here
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const license = typeof pkg.license === "string" ? pkg.license
      : Array.isArray(pkg.licenses) ? pkg.licenses.map((l) => l.type || l).join(" OR ")
      : entry.license || "UNKNOWN";
    out.push({
      name: pkg.name || path.slice("node_modules/".length),
      version: pkg.version || entry.version || "",
      license,
      url: repoUrl(pkg.repository) || pkg.homepage || `https://www.npmjs.com/package/${pkg.name}`,
      texts: licenseFilesIn(dir).map(addText).filter(Boolean),
    });
  }
  return out;
}

// ── Rust crates ─────────────────────────────────────────────────────────────

function cargoPackages() {
  const raw = execFileSync(
    "cargo",
    // Not --locked: a release build stamps its version into Cargo.toml first,
    // which only cargo's own build then writes into Cargo.lock.
    ["metadata", "--format-version", "1", "--manifest-path", join(ROOT, "src-tauri", "Cargo.toml")],
    { maxBuffer: 512 * 1024 * 1024, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  const meta = JSON.parse(raw);
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const root = meta.resolve.root;
  const isProcMacro = (p) => p.targets.some((t) => t.kind.includes("proc-macro"));

  const shipped = new Set();
  const stack = [root];
  while (stack.length) {
    const id = stack.pop();
    if (shipped.has(id)) continue;
    shipped.add(id);
    for (const d of nodes.get(id)?.deps || []) {
      if (!d.dep_kinds.some((k) => k.kind === null)) continue; // build / dev only
      const p = pkgs.get(d.pkg);
      if (!p || isProcMacro(p)) continue;
      stack.push(d.pkg);
    }
  }
  shipped.delete(root);

  const list = [...shipped].map((id) => pkgs.get(id));
  const own = new Map(list.map((p) => [p.id, licenseFilesIn(dirname(p.manifest_path), p.license_file)]));

  // Standard texts for crates that carry none. Apache-2.0 and MPL-2.0 read
  // the same for everyone, so take the copy most other crates here carry;
  // MIT and BSD-3-Clause name their authors, so those are written out with
  // the crate's own.
  const SAME_FOR_ALL = new Set(["Apache-2.0", "MPL-2.0"]);
  const common = new Map(); // licence → normalized text → { n, text }
  for (const p of list) {
    const lic = (p.license || "").trim();
    if (!SAME_FOR_ALL.has(lic)) continue;
    const seen = common.get(lic) || new Map();
    common.set(lic, seen);
    for (const t of own.get(p.id)) {
      const key = t.replace(/\s+/g, " ").trim();
      const hit = seen.get(key) || { n: 0, text: t };
      hit.n++;
      seen.set(key, hit);
    }
  }
  const standard = (id) =>
    [...(common.get(id)?.values() || [])].sort((a, b) => b.n - a.n)[0]?.text || null;
  const MIT = (authors) =>
    `MIT License\n\nCopyright (c) ${authors}\n\n` +
    "Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated " +
    "documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation " +
    "the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to " +
    "permit persons to whom the Software is furnished to do so, subject to the following conditions:\n\n" +
    "The above copyright notice and this permission notice shall be included in all copies or substantial portions of " +
    "the Software.\n\n" +
    "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO " +
    "THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE " +
    "AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, " +
    "TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE " +
    "SOFTWARE.";
  const BSD3 = (authors) =>
    `BSD 3-Clause License\n\nCopyright (c) ${authors}\nAll rights reserved.\n\n` +
    "Redistribution and use in source and binary forms, with or without modification, are permitted provided that the " +
    "following conditions are met:\n\n" +
    "1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following " +
    "disclaimer.\n\n" +
    "2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the " +
    "following disclaimer in the documentation and/or other materials provided with the distribution.\n\n" +
    "3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote " +
    "products derived from this software without specific prior written permission.\n\n" +
    "THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS \"AS IS\" AND ANY EXPRESS OR IMPLIED WARRANTIES, " +
    "INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE " +
    "DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, " +
    "SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR " +
    "SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, " +
    "WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE " +
    "OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.";

  return list.map((p) => {
    let found = own.get(p.id);
    if (found.length === 0) {
      const authors = (p.authors || []).map((a) => a.replace(/\s*<[^>]*>/, "")).join(", ") || `the ${p.name} authors`;
      const ids = (p.license || "").split(/\s+OR\s+|\//).map((s) => s.replace(/[()]/g, "").trim());
      const pick = ids.includes("MIT") ? MIT(authors)
        : ids.map(standard).find(Boolean) || (ids.includes("BSD-3-Clause") ? BSD3(authors) : null);
      found = pick ? [pick] : [];
    }
    return {
      name: p.name,
      version: p.version,
      license: p.license || (p.license_file ? "See its licence file" : "UNKNOWN"),
      url: p.repository || p.homepage || `https://crates.io/crates/${p.name}`,
      texts: found.map(addText).filter(Boolean),
    };
  });
}

// ── Write ───────────────────────────────────────────────────────────────────

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true });
const groups = [
  { id: "app", title: "Submarine", packages: [{
    name: "Submarine", version: JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version,
    license: "MIT", url: "https://github.com/SinaXhpm/Submarine",
    texts: [addText(readFileSync(join(ROOT, "LICENSE"), "utf8"))].filter(Boolean),
  }] },
  { id: "npm", title: "JavaScript packages", packages: npmPackages().sort(byName) },
];
let cargoNote = null;
try {
  groups.push({ id: "cargo", title: "Rust crates", packages: cargoPackages().sort(byName) });
} catch (e) {
  if (process.env.CI) throw e;
  cargoNote = String(e.message || e).split("\n")[0];
}

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, JSON.stringify({ groups, texts: Object.fromEntries(texts) }));
const count = groups.reduce((n, g) => n + g.packages.length, 0);
const missing = groups.flatMap((g) => g.packages.filter((p) => p.texts.length === 0).map((p) => `${p.name} ${p.version} (${p.license})`));
console.log(`notices: ${count} packages, ${texts.size} licence texts → ${OUT.slice(ROOT.length + 1)} (${Math.round(statSync(OUT).size / 1024)} KB)`);
if (missing.length) console.warn(`notices: no licence text for ${missing.join(", ")}`);
if (cargoNote) console.warn(`notices: left out the Rust crates — cargo metadata failed: ${cargoNote}`);

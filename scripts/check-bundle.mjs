// Fails when the built page assigns to a name that's declared nowhere. The
// page is an ES module, so such an assignment throws "x is not defined" when
// it runs, and it's the minifier that can bring one in: with Vite's old
// default target (ES2020), esbuild rewrote xterm.js's `let r; … (r ||= {})`
// as `(void 0 || (r = {}))` and dropped the `let`, so xterm threw at vim's
// first mode query and that terminal drew nothing more.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse } from "@babel/parser";
import traverseModule from "@babel/traverse";

const traverse = traverseModule.default ?? traverseModule;
const dir = process.argv[2] ?? "dist/assets";

const problems = [];
for (const file of readdirSync(dir).filter((f) => f.endsWith(".js"))) {
  const src = readFileSync(join(dir, file), "utf8");
  traverse(parse(src, { sourceType: "module" }), {
    "AssignmentExpression|UpdateExpression"(path) {
      const target = path.node.left ?? path.node.argument;
      if (target.type !== "Identifier" || path.scope.hasBinding(target.name, true)) return;
      const { line, column } = target.loc.start;
      const around = src.slice(Math.max(0, path.node.start - 60), path.node.end + 40);
      problems.push(`${file}:${line}:${column + 1}: "${target.name}" is assigned but never declared: …${around}…`);
    },
  });
}
if (problems.length) {
  console.error(`The built page assigns to names it never declares; each throws when it runs:\n${problems.join("\n")}`);
  process.exit(1);
}
console.log(`check-bundle: no assignment to an undeclared name in ${dir}.`);

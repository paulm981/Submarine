import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// The page's tests (from the repository root: npm --prefix tools/test test).
// They live here, with vitest in a package of their own, so the original
// project's package.json and lockfile stay as they are. They run in Node,
// with no page and no Tauri, from the repository root, so the app's own
// modules and dependencies resolve as they do in the app, and "/src/…"
// (fork-hooks.test.ts) means the app's src.
export default defineConfig({
  root: fileURLToPath(new URL("../..", import.meta.url)),
  test: {
    include: ["tools/test/**/*.test.ts"],
    environment: "node",
  },
});

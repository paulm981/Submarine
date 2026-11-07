import { defineConfig } from "vitest/config";

// The frontend's tests (npm test): pure logic in the fork's own files, and
// src/fork-hooks.test.ts. They run in Node, with no page and no Tauri, so
// they take a second or two. A file of its own, so vite.config.ts (the
// original project's) stays as it is.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});

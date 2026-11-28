import { describe, expect, it } from "vitest";

const sources = import.meta.glob<string>("/src/fork/desktop/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

const codeOf = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/.*$/gm, "$1");

const staticImportsOf = (s: string): string[] => {
  const out: string[] = [];
  for (const m of codeOf(s).matchAll(/\b(?:import|export)\s+(?!type\b)[\s\S]*?\bfrom\s*["']([^"']+)["']/g)) {
    out.push(m[1]);
  }
  return out;
};

describe("startup initial chunk stays slim (#285)", () => {
  it("entry desktop modules never statically import unlocked components or the regex highlight engine", () => {
    const forbidden = [
      "HomePane",
      "TerminalView",
      "SessionTree",
      "AiPanel",
      "AiSidePane",
      "AiTrust",
      "AllTabsMenu",
      "PingPanel",
      "BulkDialog",
      "broadcastTiles",
      "ui/highlight",
    ];
    for (const file of ["index.ts", "parts.tsx", "hooks.ts", "settings.ts"]) {
      const text = sources[`/src/fork/desktop/${file}`];
      expect(text, file).toBeTypeOf("string");
      const specs = staticImportsOf(text);
      for (const bad of forbidden) {
        expect(
          specs.filter((sp) => sp.includes(bad) && !sp.includes("highlightPrefs")),
          `${file} must not statically import ${bad}`,
        ).toEqual([]);
      }
    }
  });
});

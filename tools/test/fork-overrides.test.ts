// Where the fork restyles an element of the original project's, the
// original project's own attributes stay as they are and the fork's are
// spread after them (`{...Fork.paneLook(…)}`), which win. That keeps those
// lines out of the fork's footprint, but a sync that changes them would
// merge without a conflict and be overridden without anyone noticing. So
// this records, for each spread, the original project's lines just above
// it, and fails when a sync changes them: look at what the original
// project changed (a class that does something, not only how it looks,
// such as hiding a pane), carry it into the fork's version in
// src/fork/session if it's wanted, then update the lines here.
//
// Lines are compared trimmed, so re-indenting alone doesn't count.

import { describe, expect, it } from "vitest";

const OVERRIDES: { file: string; spread: string; upstream: string[] }[] = [
  {
    file: "src/components/SessionView.tsx",
    spread: "Fork.stripLook",
    upstream: ['className="flex items-center gap-1 overflow-x-auto no-scrollbar flex-1 mr-1 sm:mr-4 mask-fade-right"'],
  },
  {
    file: "src/components/SessionView.tsx",
    spread: "Fork.termTabProps",
    upstream: ['<div key={term.id} className="group relative flex items-center"'],
  },
  {
    file: "src/components/SessionView.tsx",
    spread: "Fork.termTabLook",
    upstream: [
      'title={term.container ? t("Container: {name}", { name: term.container.name }) : undefined}',
      "className={`h-10 sm:h-8 px-3 sm:px-4 ${terminals.length > 1 ? 'pr-8 sm:pr-8' : ''} rounded-lg flex items-center gap-2 text-[11px] font-bold uppercase tracking-wider transition-all ${",
      "isFocusedHalf",
      "? 'bg-primary/10 text-primary border border-primary/20 shadow-inner'",
      ": isSplitPartner",
      "? 'bg-primary/[0.04] text-primary/70 border border-primary/10'",
      ": 'text-zinc-300 bg-white/[0.04] border border-white/10 hover:bg-white/[0.08] hover:border-white/20 hover:text-white'",
      "} ${term.container ? 'ring-1 ring-sky-400/30' : ''} ${term.ai && aiFull ? 'ring-1 ring-rose-500/60' : ''}`}",
    ],
  },
  {
    file: "src/components/SessionView.tsx",
    spread: "Fork.termCloseLook",
    upstream: [
      'className="absolute right-0 sm:right-1 top-1/2 -translate-y-1/2 w-8 h-10 sm:w-5 sm:h-5 flex items-center justify-center rounded opacity-100 sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100 hover:bg-white/5 hover:text-red-400 text-zinc-500 transition-all"',
    ],
  },
  {
    file: "src/components/SessionView.tsx",
    spread: "Fork.paneLook",
    upstream: [
      "className={`absolute ${",
      "inSplit",
      "? `z-10 border ${",
      "isFocused",
      '? "border-primary/40 shadow-[inset_0_0_0_1px_rgba(var(--primary),0.15)]"',
      ': "border-white/[0.03]"',
      "}`",
      ": inView ? 'inset-0 opacity-100 z-10' : 'out-of-view z-0'",
      "}`}",
    ],
  },
];

const SOURCES = import.meta.glob<string>("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true });

describe("the original project's lines the fork's restyles override", () => {
  for (const { file, spread, upstream } of OVERRIDES) {
    it(`${file}: ${spread}`, () => {
      const lines = (SOURCES[`/${file}`] ?? "").split("\n");
      const at = lines.findIndex((l) => l.includes(`{...${spread}(`));
      expect(at, `${file} no longer spreads ${spread}`).toBeGreaterThanOrEqual(0);
      const above = lines.slice(Math.max(0, at - upstream.length), at).map((l) => l.trim());
      expect(above, `the original project changed what ${spread} overrides: check the fork's version, then update this test`).toEqual(upstream);
    });
  }
});

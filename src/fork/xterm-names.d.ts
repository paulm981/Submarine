// The fork's terminal is on xterm.js 6 (@xterm/*). The original project's AI
// terminal (src/components/ai/AiTerminal.tsx) still names the old packages:
// to TypeScript they are these, as vite.config.ts's `resolve.alias` makes
// them for the build.
declare module "xterm" {
  export * from "@xterm/xterm";
}
declare module "xterm-addon-fit" {
  export * from "@xterm/addon-fit";
}

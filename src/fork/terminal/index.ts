// The fork's code for a terminal. src/components/TerminalView.tsx is the
// original project's file, so it reaches all of this through one import
// (`* as Fork`) and calls it in one-line `Fork.…` hooks (#163);
// tools/test/fork-hooks.test.ts checks they're all still there.
export * from "./rules";
export * from "./hooks";
export * from "./mount";
export * from "./parts";
export { broadcastWrite } from "./broadcast";
export { fileMenuAt } from "./fileMenu";
// Fork modules TerminalView calls directly, through the same import.
export { appHotkey } from "../../ui/terminalDock";
export { schemeForServer } from "../../ui/terminalThemes";
export { readClipboardText } from "../../util/clipboard";

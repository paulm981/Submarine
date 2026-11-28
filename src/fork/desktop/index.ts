// The fork's code for the main window. src/DesktopApp.tsx is the original
// project's file, so it reaches all of this through one import (`* as Fork`)
// and calls it in one-line `Fork.…` hooks (#163); tools/test/fork-hooks.test.ts
// checks they're all still there.
import { fromForkWorkspace } from "./lazy";

export * from "./rules";
export * from "./settings";
export * from "./hooks";
export * from "./parts";
export * from "./broadcastWall";
export const BroadcastBar = fromForkWorkspace("BroadcastBar");
export const BroadcastTab = fromForkWorkspace("BroadcastTab");
export const BroadcastTileChips = fromForkWorkspace("BroadcastTileChips");
// Fork modules DesktopApp renders or reads directly, through the same import.
export { mayBeShortcut, tabStepList } from "../keys";
export const AiTrustModal = fromForkWorkspace("AiTrustModal");
export { TerminalTurn } from "../terminal/turns";
export const PingPanel = fromForkWorkspace("PingPanel");
// Bulk push and pull (#159): the dialog, and its items in a tab's menu.
export const BulkTransfer = fromForkWorkspace("BulkTransfer");
export { BulkTabItems } from "../bulk/menus";
export { COMPACT_TAB_NAME, COMPACT_TAB_PAD, NewsDot } from "../../ui/tabStyle";
// The original project's "bold in bright colors" and "keep text readable",
// which the fork's terminal colors (ui/terminalThemes) read and keep.
export { BOLD_BRIGHT_KEY, MIN_CONTRAST_KEY, readBoldBright, readMinContrast } from "../../ui/terminalThemes";

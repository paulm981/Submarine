// The fork's code for a server's tab. src/components/SessionView.tsx is the
// original project's file, so it reaches all of this through one import
// (`* as Fork`) and calls it in one-line `Fork.…` hooks (#163);
// tools/test/fork-hooks.test.ts checks they're all still there.
export * from "./rules";
export * from "./hooks";
export * from "./split";
export * from "./parts";
// Fork modules SessionView renders directly, through the same import.
export { KeptSessions } from "../../components/KeptSessions";
export { TerminalTurn } from "../terminal/turns";

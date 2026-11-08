// The fork's code for the Profile panel. src/components/ProfilePanel.tsx is
// the original project's file, so it reaches all of this through one import
// (`* as Fork`) and calls it in one-line `Fork.…` hooks (#163);
// tools/test/fork-hooks.test.ts checks they're all still there.
export * from "./hooks";
export * from "./parts";

// Types for corpus.mjs, for the page's tests (tools/test/perf.test.ts).
export function corpus(kind: "plain" | "log" | "sgr" | "ls" | "yes" | "mixed", bytes: number, seed?: number): Uint8Array;
export function topFrame(frame: number, rows?: number): Uint8Array;
export function batches(data: Uint8Array, size?: number): Uint8Array[];

// Unicode controls that reorder or hide text: the left-to-right /
// right-to-left marks, overrides and isolates, and the zero-width characters.
// A server can put them in a file name so that "Invoice_<U+202E>fdp.exe"
// reads "Invoice_exe.pdf". Names on screen drop them, which leaves the
// characters in their real order; every operation still uses the real name.
const HIDDEN_CONTROLS = /[\u200B-\u200F\u202A-\u202E\u2060-\u2069\uFEFF]/g;

export const visibleName = (name: string): string => name.replace(HIDDEN_CONTROLS, "");

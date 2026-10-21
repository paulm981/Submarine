// Unicode characters that reorder or hide text: the format characters
// (\p{Cf}: the left-to-right / right-to-left marks, overrides and isolates,
// the zero-width characters, the soft hyphen, the tag characters), the line
// and paragraph separators, and the ones that draw nothing though they're
// letters or marks (the combining grapheme joiner, the Hangul fillers, the
// Khmer inherent vowels, the variation selectors). A server can put them in a
// file name so that "Invoice_<U+202E>fdp.exe" reads "Invoice_exe.pdf". Names
// on screen drop them, which leaves the characters in their real order;
// every operation still uses the real name.
export const HIDDEN_CONTROLS =
  /[\p{Cf}\p{Zl}\p{Zp}­͏ᅟᅠ឴឵᠎ㅤﾠ︀-️\u{E0000}-\u{E007F}\u{E0100}-\u{E01EF}]/gu;

export const visibleName = (name: string): string => name.replace(HIDDEN_CONTROLS, "");

// A colour scheme of the user's own, pasted as a Windows Terminal colour
// scheme (the JSON most scheme sites offer), from the original project
// (#183). It is checked key by key: only colours in #rgb / #rrggbb /
// #rrggbbaa form are taken.
//
// The fork keeps its schemes, this device's choice and a server's own scheme
// in src/ui/terminalThemes.ts; this file only reads and writes the pasted one.
import type { ITheme } from "@xterm/xterm";

export const CUSTOM_THEME_ID = "custom";

export type PastedScheme = { id: string; name: string; theme: ITheme };

// The ANSI colour names, in order, as xterm and Windows Terminal call them
// ("magenta" is "purple" in a Windows Terminal scheme).
const ANSI = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

// A Windows Terminal colour scheme, pasted as JSON: { "name", "background",
// "foreground", "cursorColor", "selectionBackground", "black" … "brightWhite" }
// ("purple" / "brightPurple" for magenta). One scheme, or a list of them (the
// first is used). Throws a message that says what's wrong.
export function parseWindowsTerminalScheme(text: string): PastedScheme {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error("That isn't JSON. Paste a Windows Terminal colour scheme: { \"name\": …, \"background\": \"#…\", … }");
  }
  if (Array.isArray(data)) data = data[0];
  if (!data || typeof data !== "object") throw new Error("That isn't a colour scheme.");
  const src = data as Record<string, unknown>;
  const color = (key: string, required: boolean): string | undefined => {
    const v = src[key];
    if (v === undefined && !required) return undefined;
    if (typeof v !== "string" || !HEX.test(v.trim())) throw new Error(`"${key}" needs a colour like #1e1e2e.`);
    return v.trim().toLowerCase();
  };
  const background = color("background", true)!;
  const theme: ITheme = { background, foreground: color("foreground", true)!, cursorAccent: background };
  const cursor = color("cursorColor", false);
  if (cursor) theme.cursor = cursor;
  const selection = color("selectionBackground", false);
  // Windows Terminal draws its selection see-through; an opaque one here
  // would hide the text under it.
  if (selection) theme.selectionBackground = selection.length === 7 ? `${selection}55` : selection;
  for (const k of ANSI) {
    const wt = k === "magenta" ? "purple" : k === "brightMagenta" ? "brightPurple" : k;
    (theme as Record<string, string | undefined>)[k] = color(wt, true);
  }
  const rawName = typeof src.name === "string" ? src.name.replace(/[\u0000-\u001f\u007f]/g, "").trim() : "";
  return { id: CUSTOM_THEME_ID, name: rawName.slice(0, 60) || "My scheme", theme };
}

// A checked scheme back as Windows Terminal JSON, to keep: only the keys it
// was read from, whatever else the pasted text had.
export function schemeToWindowsTerminalJson(s: PastedScheme): string {
  const t = s.theme as Record<string, string | undefined>;
  const out: Record<string, string> = { name: s.name };
  if (t.background) out.background = t.background;
  if (t.foreground) out.foreground = t.foreground;
  if (t.cursor) out.cursorColor = t.cursor;
  if (t.selectionBackground) out.selectionBackground = t.selectionBackground;
  for (const k of ANSI) {
    const wt = k === "magenta" ? "purple" : k === "brightMagenta" ? "brightPurple" : k;
    if (t[k]) out[wt] = t[k]!;
  }
  return JSON.stringify(out);
}

import type { ITheme } from "@xterm/xterm";
import { IS_WINDOWS } from "../util/platform";
import { CUSTOM_THEME_ID, parseWindowsTerminalScheme } from "../util/terminalTheme";

// ─────────────────────────────────────────────────────────────────────────────
// Terminal color schemes.
//
// "Submarine" is the app's own look. The others are well-known open-source
// schemes, their colors taken from each project's own published files (or the
// copy Windows Terminal ships). Each keeps its credit and licence: the texts
// ship with the app under /licenses/color-schemes/ and Settings › Open-source
// licenses lists them.
//
// The scheme is a per-device preference (Settings › Terminal), and a server can
// have its own (its edit panel) — e.g. a light one for production. A server's
// choice syncs with it; see `terminal_theme` in src-tauri/src/lib.rs.
//
// This device can also use a scheme of the user's own, pasted as Windows
// Terminal JSON (the original project's #183, util/terminalTheme). It stays on
// this device, so a server's own scheme is always one of the schemes here.
// ─────────────────────────────────────────────────────────────────────────────

export interface SchemeCredit {
  by: string;
  license: string;
  /** Its licence text, under /licenses/color-schemes/. */
  licenseFile: string;
  url: string;
}

export interface TerminalScheme {
  id: string;
  name: string;
  light: boolean;
  credit: SchemeCredit | null;
  theme: ITheme;
}

export const DEFAULT_SCHEME = "submarine";
export const SCHEME_KEY = "submarine-terminal-scheme";

export const SCHEMES: readonly TerminalScheme[] = [
  {
    id: DEFAULT_SCHEME, name: "Submarine", light: false, credit: null,
    // The look terminals always had; the 16 colors are xterm.js's own.
    theme: { background: "#09090b", foreground: "#e4e4e7", cursor: "#60a5fa", selectionBackground: "rgba(96, 165, 250, 0.3)" },
  },
  {
    id: "campbell", name: "Campbell", light: false,
    credit: { by: "Microsoft — Windows Terminal's default", license: "MIT", licenseFile: "microsoft-terminal.txt", url: "https://github.com/microsoft/terminal" },
    theme: { background: "#0c0c0c", foreground: "#cccccc", cursor: "#ffffff", black: "#0c0c0c", red: "#c50f1f", green: "#13a10e", yellow: "#c19c00", blue: "#0037da", magenta: "#881798", cyan: "#3a96dd", white: "#cccccc", brightBlack: "#767676", brightRed: "#e74856", brightGreen: "#16c60c", brightYellow: "#f9f1a5", brightBlue: "#3b78ff", brightMagenta: "#b4009e", brightCyan: "#61d6d6", brightWhite: "#f2f2f2" },
  },
  {
    id: "one-half-dark", name: "One Half Dark", light: false,
    credit: { by: "Son A. Pham, as Windows Terminal ships it", license: "MIT", licenseFile: "one-half.txt", url: "https://github.com/sonph/onehalf" },
    theme: { background: "#282c34", foreground: "#dcdfe4", cursor: "#ffffff", black: "#282c34", red: "#e06c75", green: "#98c379", yellow: "#e5c07b", blue: "#61afef", magenta: "#c678dd", cyan: "#56b6c2", white: "#dcdfe4", brightBlack: "#5a6374", brightRed: "#e06c75", brightGreen: "#98c379", brightYellow: "#e5c07b", brightBlue: "#61afef", brightMagenta: "#c678dd", brightCyan: "#56b6c2", brightWhite: "#dcdfe4" },
  },
  {
    id: "one-half-light", name: "One Half Light", light: true,
    credit: { by: "Son A. Pham, as Windows Terminal ships it", license: "MIT", licenseFile: "one-half.txt", url: "https://github.com/sonph/onehalf" },
    theme: { background: "#fafafa", foreground: "#383a42", cursor: "#4f525d", selectionBackground: "#383a42", black: "#383a42", red: "#e45649", green: "#50a14f", yellow: "#c18301", blue: "#0184bc", magenta: "#a626a4", cyan: "#0997b3", white: "#fafafa", brightBlack: "#4f525d", brightRed: "#df6c75", brightGreen: "#98c379", brightYellow: "#e4c07a", brightBlue: "#61afef", brightMagenta: "#c577dd", brightCyan: "#56b5c1", brightWhite: "#ffffff" },
  },
  {
    id: "solarized-dark", name: "Solarized Dark", light: false,
    credit: { by: "Ethan Schoonover, as Windows Terminal ships it", license: "MIT", licenseFile: "solarized.txt", url: "https://github.com/altercation/solarized" },
    theme: { background: "#002b36", foreground: "#839496", cursor: "#ffffff", black: "#002b36", red: "#dc322f", green: "#859900", yellow: "#b58900", blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5", brightBlack: "#073642", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3" },
  },
  {
    id: "solarized-light", name: "Solarized Light", light: true,
    credit: { by: "Ethan Schoonover, as Windows Terminal ships it", license: "MIT", licenseFile: "solarized.txt", url: "https://github.com/altercation/solarized" },
    theme: { background: "#fdf6e3", foreground: "#657b83", cursor: "#002b36", selectionBackground: "#2c4d57", black: "#002b36", red: "#dc322f", green: "#859900", yellow: "#b58900", blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5", brightBlack: "#073642", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83", brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3" },
  },
  {
    id: "dracula", name: "Dracula", light: false,
    credit: { by: "Dracula Theme", license: "MIT", licenseFile: "dracula.txt", url: "https://github.com/dracula/windows-terminal" },
    theme: { background: "#282a36", foreground: "#f8f8f2", cursor: "#f8f8f2", selectionBackground: "#44475a", black: "#21222c", red: "#ff5555", green: "#50fa7b", yellow: "#f1fa8c", blue: "#bd93f9", magenta: "#ff79c6", cyan: "#8be9fd", white: "#f8f8f2", brightBlack: "#6272a4", brightRed: "#ff6e6e", brightGreen: "#69ff94", brightYellow: "#ffffa5", brightBlue: "#d6acff", brightMagenta: "#ff92df", brightCyan: "#a4ffff", brightWhite: "#ffffff" },
  },
  {
    id: "nord", name: "Nord", light: false,
    credit: { by: "Sven Greb; palette from iTerm2-Color-Schemes", license: "MIT", licenseFile: "nord.txt", url: "https://www.nordtheme.com" },
    theme: { background: "#2e3440", foreground: "#d8dee9", cursor: "#eceff4", selectionBackground: "#eceff4", black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b", blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0", brightBlack: "#596377", brightRed: "#bf616a", brightGreen: "#a3be8c", brightYellow: "#ebcb8b", brightBlue: "#81a1c1", brightMagenta: "#b48ead", brightCyan: "#8fbcbb", brightWhite: "#eceff4" },
  },
  {
    id: "tokyo-night", name: "Tokyo Night", light: false,
    credit: { by: "Folke Lemaitre (tokyonight.nvim)", license: "Apache-2.0", licenseFile: "tokyo-night.txt", url: "https://github.com/folke/tokyonight.nvim" },
    theme: { background: "#1a1b26", foreground: "#c0caf5", cursor: "#c0caf5", selectionBackground: "#283457", black: "#15161e", red: "#f7768e", green: "#9ece6a", yellow: "#e0af68", blue: "#7aa2f7", magenta: "#bb9af7", cyan: "#7dcfff", white: "#a9b1d6", brightBlack: "#414868", brightRed: "#f7768e", brightGreen: "#9ece6a", brightYellow: "#e0af68", brightBlue: "#7aa2f7", brightMagenta: "#9d7cd8", brightCyan: "#7dcfff", brightWhite: "#c0caf5" },
  },
  {
    id: "catppuccin-mocha", name: "Catppuccin Mocha", light: false,
    credit: { by: "Catppuccin", license: "MIT", licenseFile: "catppuccin.txt", url: "https://github.com/catppuccin/windows-terminal" },
    theme: { background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", selectionBackground: "#585b70", black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af", blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de", brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1", brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7", brightCyan: "#94e2d5", brightWhite: "#a6adc8" },
  },
  {
    id: "catppuccin-latte", name: "Catppuccin Latte", light: true,
    credit: { by: "Catppuccin", license: "MIT", licenseFile: "catppuccin.txt", url: "https://github.com/catppuccin/windows-terminal" },
    theme: { background: "#eff1f5", foreground: "#4c4f69", cursor: "#dc8a78", selectionBackground: "#acb0be", black: "#5c5f77", red: "#d20f39", green: "#40a02b", yellow: "#df8e1d", blue: "#1e66f5", magenta: "#ea76cb", cyan: "#179299", white: "#acb0be", brightBlack: "#acb0be", brightRed: "#d20f39", brightGreen: "#40a02b", brightYellow: "#df8e1d", brightBlue: "#1e66f5", brightMagenta: "#ea76cb", brightCyan: "#179299", brightWhite: "#bcc0cc" },
  },
  {
    id: "gruvbox-dark", name: "Gruvbox Dark", light: false,
    credit: { by: "Pavel Pertsev; palette from iTerm2-Color-Schemes", license: "MIT", licenseFile: "iterm2-color-schemes.txt", url: "https://github.com/morhetz/gruvbox" },
    theme: { background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", selectionBackground: "#665c54", black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921", blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984", brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26", brightYellow: "#fabd2f", brightBlue: "#83a598", brightMagenta: "#d3869b", brightCyan: "#8ec07c", brightWhite: "#ebdbb2" },
  },
];

const byId = new Map(SCHEMES.map((s) => [s.id, s]));

// ── A scheme of the user's own ──────────────────────────────────────────────

export const CUSTOM_SCHEME = CUSTOM_THEME_ID;
/** The pasted scheme, as checked Windows Terminal JSON. */
export const CUSTOM_SCHEME_KEY = "submarine-terminal-custom-theme";

// Parsed once per text, so the same scheme object comes back each time and a
// terminal sees no change when there is none.
let customCache: { raw: string; scheme: TerminalScheme | null } | null = null;

/** The user's own scheme from its Windows Terminal JSON; null for none or a broken one. */
export function customSchemeFrom(raw: string | null | undefined): TerminalScheme | null {
  if (!raw) return null;
  if (customCache?.raw === raw) return customCache.scheme;
  let scheme: TerminalScheme | null = null;
  try {
    const p = parseWindowsTerminalScheme(raw);
    scheme = { ...p, light: isLight(p.theme.background), credit: null };
  } catch { /* broken: none */ }
  customCache = { raw, scheme };
  return scheme;
}

/** This device's own scheme (Settings › Terminal), if it has one. */
export function readCustomScheme(): TerminalScheme | null {
  try {
    return customSchemeFrom(localStorage.getItem(CUSTOM_SCHEME_KEY));
  } catch {
    return null;
  }
}

function isLight(hex: string | undefined): boolean {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})/i.exec(hex ?? "");
  if (!m) return false;
  const h = m[1].length === 3 ? m[1].replace(/./g, "$&$&") : m[1];
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
  return 0.299 * r + 0.587 * g + 0.114 * b > 150;
}

/** The scheme with this id, or the default for an unknown / empty one.
 *  "custom" is this device's own scheme, while it has one. */
export function schemeById(id: string | null | undefined): TerminalScheme {
  if (id === CUSTOM_SCHEME) return readCustomScheme() || byId.get(DEFAULT_SCHEME)!;
  return (id && byId.get(id)) || byId.get(DEFAULT_SCHEME)!;
}

/** This device's chosen scheme id (Settings › Terminal). */
export function readSchemeId(): string {
  try {
    const id = localStorage.getItem(SCHEME_KEY) || DEFAULT_SCHEME;
    if (id === CUSTOM_SCHEME) return readCustomScheme() ? id : DEFAULT_SCHEME;
    return byId.has(id) ? id : DEFAULT_SCHEME;
  } catch {
    return DEFAULT_SCHEME;
  }
}

// Each saved server's own scheme, kept current by DesktopApp whenever the
// server list loads; a terminal looks its server up here, so an edit or a
// synced change recolors its open terminals without reopening them.
const serverSchemes = new Map<number, string>();

export function setServerSchemes(servers: { id: number; terminal_theme?: string | null }[]): boolean {
  const next = new Map<number, string>();
  for (const s of servers) if (s.terminal_theme && byId.has(s.terminal_theme)) next.set(s.id, s.terminal_theme);
  const same = next.size === serverSchemes.size && [...next].every(([k, v]) => serverSchemes.get(k) === v);
  if (same) return false;
  serverSchemes.clear();
  for (const [k, v] of next) serverSchemes.set(k, v);
  return true;
}

/** The scheme a terminal of this server uses: its own, else this device's. */
export function schemeForServer(serverId: number | undefined): TerminalScheme {
  return schemeById((serverId && serverSchemes.get(serverId)) || readSchemeId());
}

// ── Other terminal preferences (per device) ─────────────────────────────────

// A new key: build 228 stored "on" for everyone under the old one, before
// the default became per-platform (readGpu).
export const GPU_KEY = "submarine-terminal-gpu-v2";
export const MIN_CONTRAST_KEY = "submarine-terminal-min-contrast";
export const BOLD_BRIGHT_KEY = "submarine-terminal-bold-bright";

/** WCAG AA's ratio for body text, the one VS Code's terminal defaults to. */
export const MIN_CONTRAST_RATIO = 4.5;

const readFlag = (key: string, fallback: boolean): boolean => {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "on";
  } catch {
    return fallback;
  }
};

/** Draw with the GPU (xterm's WebGL renderer). On by default on Windows
 *  only: in WebKit's web views (WebKitGTK on Linux, and macOS's) a WebGL
 *  terminal that isn't the one being typed in stops showing new output —
 *  a split pane's `tail -f`, a Wall's tiles — until something else redraws
 *  it. Chromium's (WebView2 on Windows) draws them fine. */
export const readGpu = (): boolean => readFlag(GPU_KEY, IS_WINDOWS);
/** Lift text a program colored too close to the background. Defaults on. */
export const readMinContrast = (): boolean => readFlag(MIN_CONTRAST_KEY, true);
/** Bold text in one of the first eight colors uses its bright version, as
 *  most terminals do (from the original project, #183). Defaults on. */
export const readBoldBright = (): boolean => readFlag(BOLD_BRIGHT_KEY, true);

// ── For previews ────────────────────────────────────────────────────────────

/** The 16 colors xterm.js uses when a scheme leaves them out (Tango). */
const XTERM_ANSI = [
  "#2e3436", "#cc0000", "#4e9a06", "#c4a000", "#3465a4", "#75507b", "#06989a", "#d3d7cf",
  "#555753", "#ef2929", "#8ae234", "#fce94f", "#729fcf", "#ad7fa8", "#34e2e2", "#eeeeec",
];
const ANSI_KEYS = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

/** A scheme's 16 ANSI colors, xterm's own filling any it leaves out. */
export function ansiOf(scheme: TerminalScheme): string[] {
  return ANSI_KEYS.map((k, i) => scheme.theme[k] || XTERM_ANSI[i]);
}

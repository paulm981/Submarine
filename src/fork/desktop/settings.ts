import { invoke } from "@tauri-apps/api/core";
import { CUSTOM_SCHEME_KEY, GPU_KEY, SCHEME_KEY, readGpu, readSchemeId } from "../../ui/terminalThemes";
import { HIGHLIGHT_KEY, readHighlightId } from "../../ui/highlight";
import { PING_SOUND_KEY, readPingSound } from "../../ui/ping";
import { INFO_FONT_SIZE_KEY, readInfoFontSize } from "../../util/infoFont";

// The fork's settings in DesktopApp's `appSettings` (#163): read with the
// original project's ones, and kept on this device with them. Per-device,
// like the rest of them. Bold in bright colors and "keep text readable" are
// the original project's settings, and stay in DesktopApp, read and kept by
// the fork's terminalThemes (exported below, in index.ts).

/** The fork's settings as this device has them, for appSettings' first value. */
export const readForkSettings = () => ({
  // Terminal colors (src/ui/terminalThemes.ts) and GPU drawing. A server's
  // own colors, when it has some, win over the scheme.
  terminalScheme: readSchemeId(),
  terminalGpu: readGpu(),
  // A scheme of this device's own, pasted as Windows Terminal JSON ("" for
  // none; the scheme "custom"), from the original project (#183).
  terminalCustomScheme: (() => { try { return localStorage.getItem(CUSTOM_SCHEME_KEY) || ""; } catch { return ""; } })(),
  // Which highlight set terminals use (src/ui/highlight.ts), unless their
  // server has its own.
  terminalHighlight: readHighlightId(),
  // Read each server's OS for its badge (server_os.rs). Defaults ON.
  detectOs: localStorage.getItem('submarine-detect-os') !== 'off',
  // Fork maintainers only: the update check also counts the original
  // project's new commits (about.rs). Defaults OFF.
  watchUpstream: localStorage.getItem('submarine-watch-upstream') === 'on',
  // Ask before closing a tab whose session is connected (Settings ›
  // Servers, or "Don't ask again" in the prompt). Defaults ON.
  confirmCloseSession: localStorage.getItem('submarine-confirm-close') !== 'off',
  // A short sound when a server being pinged answers again (Settings ›
  // Servers). Per-device; defaults ON. See ui/ping.
  pingSound: readPingSound(),
  // The Info panel's text size (Settings › UI Customization). Per-device.
  // See util/infoFont.
  infoFontSize: readInfoFontSize(),
  // Flat tabs that touch, instead of pills (title bar and each server's
  // terminal tabs). Per-device; defaults OFF.
  compactTabs: localStorage.getItem('submarine-compact-tabs') === 'on',
});

export type ForkSettings = ReturnType<typeof readForkSettings>;

/** Keeps the fork's settings on this device, before the terminals are told
 *  they changed. */
export const writeForkSettings = (s: ForkSettings) => {
  // The scheme of this device's own first, so "custom" finds it.
  if (s.terminalCustomScheme) localStorage.setItem(CUSTOM_SCHEME_KEY, s.terminalCustomScheme);
  else localStorage.removeItem(CUSTOM_SCHEME_KEY);
  localStorage.setItem(SCHEME_KEY, s.terminalScheme);
  localStorage.setItem(GPU_KEY, s.terminalGpu ? 'on' : 'off');
  localStorage.setItem(HIGHLIGHT_KEY, s.terminalHighlight);
  localStorage.setItem('submarine-detect-os', s.detectOs ? 'on' : 'off');
  localStorage.setItem('submarine-watch-upstream', s.watchUpstream ? 'on' : 'off');
  localStorage.setItem('submarine-confirm-close', s.confirmCloseSession ? 'on' : 'off');
  localStorage.setItem(PING_SOUND_KEY, s.pingSound ? 'on' : 'off');
  localStorage.setItem('submarine-compact-tabs', s.compactTabs ? 'on' : 'off');
  localStorage.setItem(INFO_FONT_SIZE_KEY, String(s.infoFontSize));
};

/** A server's own terminal colors and highlight set, saved with the rest of
 *  its form (each its own write, like the original project's run-on-connect
 *  one). */
export const saveServerLooks = async (id: number, node: { terminalTheme?: string; highlight?: string }, addLog: (msg: string, type?: string) => void) => {
  try {
    await invoke("set_server_terminal_theme", { id, value: node.terminalTheme || null });
  } catch (e) {
    addLog(`SAVE_TERMINAL_THEME_FAILED: ${e}`, "error");
  }
  try {
    await invoke("set_server_highlight", { id, value: node.highlight || null });
  } catch (e) {
    addLog(`SAVE_HIGHLIGHT_FAILED: ${e}`, "error");
  }
};

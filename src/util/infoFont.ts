import { useEffect, useState } from "react";

// The Info panel's text size (Settings › UI Customization) — per-device
// (localStorage), like the other appearance settings. The panel was drawn
// for 11px text; another size scales the whole panel, so its smaller labels,
// icons and spacing keep their proportions.
export const INFO_FONT_SIZE_KEY = "submarine-info-font-size";

export const MIN_INFO_FONT_SIZE = 10;
export const MAX_INFO_FONT_SIZE = 16;
export const DEFAULT_INFO_FONT_SIZE = 11;

export function clampInfoFontSize(v: unknown): number {
  const n = typeof v === "number" ? v : parseInt(String(v ?? ""), 10);
  if (!Number.isFinite(n)) return DEFAULT_INFO_FONT_SIZE;
  return Math.min(MAX_INFO_FONT_SIZE, Math.max(MIN_INFO_FONT_SIZE, Math.round(n)));
}

export function readInfoFontSize(): number {
  try {
    return clampInfoFontSize(localStorage.getItem(INFO_FONT_SIZE_KEY) ?? DEFAULT_INFO_FONT_SIZE);
  } catch {
    return DEFAULT_INFO_FONT_SIZE;
  }
}

/// The size, following changes made in Settings.
export function useInfoFontSize(): number {
  const [size, setSize] = useState(readInfoFontSize);
  useEffect(() => {
    const changed = () => setSize(readInfoFontSize());
    window.addEventListener("submarine-settings-changed", changed);
    return () => window.removeEventListener("submarine-settings-changed", changed);
  }, []);
  return size;
}

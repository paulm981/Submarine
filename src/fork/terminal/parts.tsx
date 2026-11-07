import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of a terminal's page, each rendered by TerminalView in
// one line (#163): TerminalView.tsx is the original project's file.
// ─────────────────────────────────────────────────────────────────────────────

/** What floats over the terminal (its search box, Saved password button and
 *  toast), drawn inside what the AI side pane moves (`el`, the dock), so they
 *  go with it. Nothing until the terminal has made its dock. */
export const TerminalOverlay = ({ el, children }: { el: HTMLDivElement | null; children: ReactNode }) =>
  el ? createPortal(children, el) : null;

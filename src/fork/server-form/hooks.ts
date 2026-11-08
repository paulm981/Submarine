import { useEffect, useRef, useState } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state in the add/edit server form, as hooks AddNodePanel calls in
// one line each (#163). Each keeps what AddNodePanel kept, with the same
// effects, called where AddNodePanel had them.
// ─────────────────────────────────────────────────────────────────────────────

/** Opened from "Pick a jump host" (a server whose jump host was deleted, see
 *  SessionView): Jump via is brought into view and focused. Gives the ref for
 *  the Jump via row. */
export const useFocusJump = (isOpen: boolean, focusJump: unknown) => {
  const jumpRowRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!isOpen || !focusJump) return;
    const t = window.setTimeout(() => {
      const row = jumpRowRef.current;
      row?.scrollIntoView({ block: "center" });
      row?.querySelector<HTMLElement>("input, button")?.focus();
    }, 150);
    return () => window.clearTimeout(t);
  }, [isOpen, focusJump]);
  return jumpRowRef;
};

/** The import modal's two of the fork's own: `skipped`, the sessions the
 *  pasted export holds that can't be imported (MobaXterm VNC…); and `done`,
 *  what the finished import did and what's left to do. While `done` is set,
 *  the modal shows it with a Done button instead of the host list. */
export const useImportResult = () => {
  const [skipped, setSkipped] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  return { skipped, setSkipped, done, setDone };
};

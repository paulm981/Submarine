import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// AI help on open sessions (#32, src-tauri/src/ai/mod.rs). The backend holds
// all of it: the AI tabs, what's shared with each, the requests waiting for
// you, what ran. The page shows it (the AI panel, Share with AI) and passes
// on your clicks; `ai-changed` says when to look again.

export interface AiServer { session_id: string; name: string }
export interface AiTab { id: string; label: string; servers: AiServer[] }
export interface AiPending {
  id: number; tab_id: string; tab_label: string; session_id: string; server: string;
  command: string; root: boolean; timeout_secs: number; asked_at: string;
}
export interface AiRunning { id: number; tab_label: string; session_id: string; server: string; command: string; started_at: string }
export interface AiDone {
  id: number; tab_label: string; server: string; command: string; root: boolean;
  outcome: "denied" | "cancelled" | "done" | "failed" | "stopped";
  exit_code: number | null; output_bytes: number; hidden: number;
  cut: boolean; timed_out: boolean; error: string | null; at: string;
}
export interface AiSnapshot {
  enabled: boolean; tabs: AiTab[]; pending: AiPending[]; running: AiRunning[]; history: AiDone[];
  audit_log: string | null;
}

const EMPTY: AiSnapshot = { enabled: false, tabs: [], pending: [], running: [], history: [], audit_log: null };

/// The AI state, kept current.
export const useAi = (): AiSnapshot => {
  const [snap, setSnap] = useState<AiSnapshot>(EMPTY);
  useEffect(() => {
    let alive = true;
    const load = () => invoke<AiSnapshot>("ai_state").then((s) => { if (alive) setSnap(s); }).catch(() => {});
    load();
    const un = listen("ai-changed", load);
    return () => { alive = false; un.then((u) => u()); };
  }, []);
  return snap;
};

/// The AI tabs a server's tab is shared with.
export const sharedWith = (snap: AiSnapshot, sessionId: string): AiTab[] =>
  snap.tabs.filter((t) => t.servers.some((s) => s.session_id === sessionId));

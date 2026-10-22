import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// AI help on open sessions (#32, src-tauri/src/ai/mod.rs). The backend holds
// all of it: the AI tabs, what's shared with each, the requests waiting for
// you, what ran. The page shows it (the AI panel, Share with AI) and passes
// on your clicks; `ai-changed` says when to look again.

export interface AiServer { session_id: string; name: string }
export interface AiTab {
  id: string; label: string; servers: AiServer[];
  /// When another program tried to connect as this AI tab (and was refused).
  intruder_at: string | null;
}
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

// One copy of the state for the whole page, however many components use it:
// one `ai_state` call per change, not one per open server tab.
let current: AiSnapshot = EMPTY;
const watchers = new Set<() => void>();
let stopListening: (() => void) | null = null;
// Only the newest answer counts, should two cross.
let asked = 0;

const load = () => {
  const n = ++asked;
  invoke<AiSnapshot>("ai_state")
    .then((s) => {
      if (n !== asked) return;
      current = s;
      watchers.forEach((w) => w());
    })
    .catch(() => {});
};

const subscribe = (w: () => void) => {
  watchers.add(w);
  if (!stopListening) {
    let alive = true;
    const un = listen("ai-changed", load);
    stopListening = () => { alive = false; un.then((u) => u()); };
    // Once listening, again: a change before then would be missed.
    void un.then(() => { if (alive) load(); });
  }
  return () => {
    watchers.delete(w);
    if (watchers.size === 0 && stopListening) {
      stopListening();
      stopListening = null;
    }
  };
};

/// The AI state, kept current.
export const useAi = (): AiSnapshot => useSyncExternalStore(subscribe, () => current);

/// The AI tabs a server's tab is shared with.
export const sharedWith = (snap: AiSnapshot, sessionId: string): AiTab[] =>
  snap.tabs.filter((t) => t.servers.some((s) => s.session_id === sessionId));

/// The AI tools this computer can start (Home's menu, under AI tools): none
/// while AI tools are off or none is installed.
export const findAiTools = (): Promise<{ id: string; label: string }[]> =>
  invoke<{ ai_tools?: { id: string; label: string }[] }>("list_local_shells")
    .then((s) => s.ai_tools ?? [])
    .catch(() => []);

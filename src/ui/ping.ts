import { useSyncExternalStore } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Ping (#33): the servers being pinged, from the session tree's or the grid's
// right-click menu. The panel (components/PingPanel.tsx) shows one card per
// server and does the checking while it's open; this is only the list, so
// the tree and the grid can add to it.
// ─────────────────────────────────────────────────────────────────────────────

export interface PingTarget {
  id: number;
  name: string;
  host: string;
  /// "ssh" (or none), "rdp", "vnc", "telnet": which port the check is about.
  protocol: string | null;
}

let pinged: PingTarget[] = [];
const listeners = new Set<() => void>();
const changed = (next: PingTarget[]) => {
  pinged = next;
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};

/// Start pinging these servers; one already being pinged stays as it is.
export const pingServers = (servers: { id: number; name?: string; host?: string; protocol?: string | null }[]) => {
  const add = servers
    .filter((s) => !pinged.some((p) => p.id === s.id))
    .map((s) => ({ id: s.id, name: s.name || s.host || "Server", host: s.host || "", protocol: s.protocol ?? null }));
  if (add.length) changed([...pinged, ...add]);
};

export const stopPing = (id: number) => changed(pinged.filter((p) => p.id !== id));
export const stopAllPings = () => { if (pinged.length) changed([]); };

export const usePinged = (): PingTarget[] => useSyncExternalStore(subscribe, () => pinged);

// The sound when a server answers again: on unless turned off under
// Settings › Servers. Per device, like the other settings.
export const PING_SOUND_KEY = "submarine-ping-sound";
export const readPingSound = (): boolean => {
  try { return localStorage.getItem(PING_SOUND_KEY) !== "off"; } catch { return true; }
};

// Two short rising notes, quiet: synthesised with Web Audio, so there's no
// sound file and it's the same on every system.
let audio: AudioContext | null = null;
export const playBackSound = () => {
  if (!readPingSound()) return;
  try {
    if (!audio) {
      const Ctor: typeof AudioContext | undefined =
        window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return;
      audio = new Ctor();
    }
    const ctx = audio;
    void ctx.resume?.();
    const t = ctx.currentTime;
    [660, 990].forEach((freq, i) => {
      const at = t + i * 0.12;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(0.08, at + 0.015);
      gain.gain.linearRampToValueAtTime(0, at + 0.14);
      osc.start(at);
      osc.stop(at + 0.15);
    });
  } catch {
    // No sound is no reason to break the panel.
  }
};

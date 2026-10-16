import { invoke } from "@tauri-apps/api/core";
import { readGpu, readMinContrast } from "../ui/terminalThemes";
import { readFontFamily, readFontSize } from "./terminalFont";

// Troubleshooting, the window's half (the log itself is src-tauri/src/applog.rs).
//
// - Errors: uncaught ones, rejected promises and console.error go to the log.
// - Freezes: a one-second timer that fires late means the window's thread was
//   busy the whole time (everything on screen runs on that one thread). A gap
//   over FREEZE_MS is logged with the terminals that were receiving the most
//   output and how much of it was still waiting to be drawn. The same timer
//   sends the backend its heartbeat, so a freeze the window never comes back
//   from is logged by the backend instead.
// - "Copy diagnostics" in Settings adds `windowSummary()` to the report.

const FREEZE_MS = 2500;
const RATE_WINDOW_MS = 10_000;

type TermStats = {
  /** Bytes received in the current RATE_WINDOW_MS. */
  received: number;
  windowStart: number;
  /** Bytes handed to the terminal that it hasn't processed yet. */
  backlog: number;
  renderer: "gpu" | "dom";
};
const terms = new Map<string, TermStats>();

const statsFor = (id: string): TermStats => {
  let s = terms.get(id);
  if (!s) {
    s = { received: 0, windowStart: Date.now(), backlog: 0, renderer: "dom" };
    terms.set(id, s);
  }
  return s;
};

/// Output for terminal `id` handed to xterm (TerminalView).
export const noteTerminalOutput = (id: string, bytes: number) => {
  const s = statsFor(id);
  const now = Date.now();
  if (now - s.windowStart > RATE_WINDOW_MS) {
    s.received = 0;
    s.windowStart = now;
  }
  s.received += bytes;
  s.backlog += bytes;
};

/// xterm finished processing `bytes` of it.
export const noteTerminalParsed = (id: string, bytes: number) => {
  const s = terms.get(id);
  if (s) s.backlog = Math.max(0, s.backlog - bytes);
};

/// How terminal `id` is drawing now: on the GPU (WebGL) or as page elements.
export const noteTerminalRenderer = (id: string, renderer: "gpu" | "dom") => {
  statsFor(id).renderer = renderer;
};

export const forgetTerminal = (id: string) => {
  terms.delete(id);
};

/// How much terminal `id` has waiting to be drawn, for its own warning.
export const terminalBacklog = (id: string): number => terms.get(id)?.backlog ?? 0;

const mb = (n: number) => (n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`);

const busiestTerminals = (): string => {
  const now = Date.now();
  const rows = [...terms.entries()]
    .filter(([, s]) => s.backlog > 0 || (s.received > 0 && now - s.windowStart <= RATE_WINDOW_MS * 2))
    .sort((a, b) => b[1].received + b[1].backlog - (a[1].received + a[1].backlog))
    .slice(0, 3)
    .map(([id, s]) => `${id} (${s.renderer}) ${mb(s.received)} received, ${mb(s.backlog)} not drawn yet`);
  return rows.length ? rows.join("; ") : "none";
};

// At most LOG_BURST lines per 10 s from the window, so an error in a loop
// can't flood the log; what's left out is counted.
const LOG_BURST = 30;
let burstStart = 0;
let burstSent = 0;
let burstDropped = 0;

export const logLine = (level: "info" | "warn" | "error", message: string) => {
  const now = Date.now();
  if (now - burstStart > 10_000) {
    if (burstDropped > 0) {
      invoke("app_log", { level: "warn", message: `${burstDropped} more lines from the window were left out` }).catch(() => {});
    }
    burstStart = now;
    burstSent = 0;
    burstDropped = 0;
  }
  if (burstSent >= LOG_BURST) {
    burstDropped++;
    return;
  }
  burstSent++;
  // Never throws: this runs inside console.error, for every caller of it.
  try {
    invoke("app_log", { level, message: message.slice(0, 4000) }).catch(() => {});
  } catch {
    /* not in the app (a test page): nowhere to log to */
  }
};

const describe = (v: unknown): string => {
  if (v instanceof Error) return `${v.name}: ${v.message}`;
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
};

const freezes: { at: Date; seconds: number }[] = [];
let started = false;

export const startDiagnostics = () => {
  if (started) return;
  started = true;

  window.addEventListener("error", (e) => {
    logLine("error", `uncaught: ${describe(e.error ?? e.message)}${e.filename ? ` (${e.filename}:${e.lineno})` : ""}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    logLine("error", `unhandled rejection: ${describe(e.reason)}`);
  });
  const consoleError = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    consoleError(...args);
    logLine("error", args.map(describe).join(" "));
  };

  const beat = () => {
    try {
      invoke("ui_heartbeat", { visible: !document.hidden }).catch(() => {});
    } catch {
      /* not in the app */
    }
  };
  let last = performance.now();
  window.setInterval(() => {
    const now = performance.now();
    const gap = now - last;
    last = now;
    // A hidden window's timers are slowed down on purpose: not a freeze.
    if (!document.hidden && gap > FREEZE_MS) {
      const seconds = gap / 1000;
      freezes.push({ at: new Date(), seconds });
      if (freezes.length > 20) freezes.shift();
      logLine("warn", `the window was stuck for ${seconds.toFixed(1)} s; busiest terminals: ${busiestTerminals()}`);
    }
    beat();
  }, 1000);
  document.addEventListener("visibilitychange", () => {
    last = performance.now();
    beat();
  });
  beat();
  logLine("info", `window started: ${navigator.userAgent}`);
};

/// One line for "Copy diagnostics": the settings that matter for drawing and
/// what the watchdog saw this session.
export const windowSummary = (): string => {
  const longest = freezes.reduce((m, f) => Math.max(m, f.seconds), 0);
  const lastFreeze = freezes[freezes.length - 1];
  const renderers = [...terms.values()].reduce(
    (acc, s) => ({ ...acc, [s.renderer]: (acc[s.renderer] ?? 0) + 1 }),
    {} as Record<string, number>,
  );
  return [
    `GPU drawing ${readGpu() ? "on" : "off"}`,
    `keep text readable ${readMinContrast() ? "on" : "off"}`,
    `font ${readFontFamily() || "default"} ${readFontSize()} px`,
    `${terms.size} terminal(s) open (${Object.entries(renderers).map(([r, n]) => `${n} ${r}`).join(", ") || "none"})`,
    `screen ${window.screen.width}x${window.screen.height} at ${window.devicePixelRatio}x`,
    freezes.length
      ? `${freezes.length} freeze(s) this session, longest ${longest.toFixed(1)} s, last at ${lastFreeze.at.toLocaleTimeString()}`
      : "no freezes seen this session",
  ].join("; ");
};

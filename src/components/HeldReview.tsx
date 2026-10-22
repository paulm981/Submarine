import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ShieldAlert, X } from "lucide-react";
import { useFocusTrap } from "../ui/confirm";

// What this device holds back in a shared profile until its user allows it:
// a change someone else made that would act on its own here (see
// src-tauri/src/approvals.rs). Opened from the notice above the server list.

type HeldServer = {
  id: number;
  name: string;
  port?: number;
  protocol?: string;
  run_on_connect?: string;
  edited_by?: string | null;
  held?: string[];
};

/** A switch of protocol first (it changes what the server is), then commands
 *  (they run in a shell), autostart, forwards, monitor commands. */
const rank = (item: string) =>
  item.startsWith("protocol:") ? -1 : item === "run_on_connect" ? 0 : item === "autostart" ? 1 : item.startsWith("tunnel:") ? 2 : 3;
const byRank = (a: string, b: string) => rank(a) - rank(b) || a.localeCompare(b);

/** A held switch to telnet or VNC: `protocol:<telnet|vnc>|<port>`. */
function switchOf(item: string): { protocol: string; label: string; port: string } | null {
  if (!item.startsWith("protocol:")) return null;
  const [protocol, port = ""] = item.slice("protocol:".length).split("|");
  return { protocol, label: protocol === "vnc" ? "VNC" : "telnet", port };
}

/** What the server is on this device while a switch is held: SSH or RDP. */
const keptLabel = (server: HeldServer) => (server.protocol === "rdp" ? "RDP" : "SSH");

/** One held item, in words, with the text it would run when there is one. */
function describe(item: string, server: HeldServer): { what: string; detail?: string } {
  if (item === "run_on_connect") {
    return { what: "Types these commands into your terminal when you connect", detail: server.run_on_connect || "" };
  }
  if (item === "autostart") {
    return { what: "Connects by itself when you unlock this profile" };
  }
  const sw = switchOf(item);
  if (sw) {
    const why = sw.protocol === "vnc"
      ? "VNC sends its password with weak protection and doesn't check the server's identity."
      : "Telnet sends everything, passwords too, as plain text.";
    const to = `${sw.label}${sw.port ? ` on port ${sw.port}` : ""}`;
    return {
      what: `Another computer changed this server to ${sw.label}. ${why}`,
      detail: `Until you choose, it stays ${keptLabel(server)} here${server.port ? `, on port ${server.port}` : ""}. Allow makes it ${to} here too. Keep it ${keptLabel(server)} changes it back to ${keptLabel(server)} on the other computers as well.`,
    };
  }
  if (item.startsWith("tunnel:")) {
    const [kind, local, remote] = item.slice("tunnel:".length).split("|");
    if (kind === "D") return { what: `SOCKS proxy on ${local}`, detail: "Programs set to use it reach the network through the server." };
    if (kind === "L") return { what: `Local forward ${local} → ${remote}`, detail: "Connections to that address on this computer go through the server to the target." };
    if (kind === "R") return { what: `Remote forward: the server listens on ${local} and sends connections to ${remote}`, detail: "Whoever reaches that port on the server reaches this side." };
    return { what: `Forward ${item.slice("tunnel:".length)}` };
  }
  if (item.startsWith("metric:")) {
    return { what: "Monitor command, run on the server at every check", detail: item.slice("metric:".length) };
  }
  return { what: item };
}

export function HeldReview({ servers, onAllow, onKeepSsh, onClose }: {
  servers: HeldServer[];
  /** `runOnConnect`: the on-connect text shown, when those commands are among the items. */
  onAllow: (serverId: number, items: string[], runOnConnect?: string) => Promise<void>;
  /** A held switch to telnet or VNC, declined: the server stays SSH (or RDP), everywhere. */
  onKeepSsh?: (serverId: number) => Promise<void>;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement | null>(null);
  useFocusTrap(1, panelRef);
  const [busy, setBusy] = useState(false);
  const held = servers.filter((s) => (s.held || []).length > 0);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); onClose(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Everything allowed: nothing left to review.
  useEffect(() => {
    if (held.length === 0) onClose();
  }, [held.length, onClose]);

  // Allowing the on-connect commands carries the text shown here, so an OK
  // never counts for a newer text that came in after it was shown.
  const allow = async (server: HeldServer, items: string[]) => {
    const commands = items.length === 0 ? (server.held || []).includes("run_on_connect") : items.includes("run_on_connect");
    setBusy(true);
    try { await onAllow(server.id, items, commands ? server.run_on_connect || "" : undefined); } finally { setBusy(false); }
  };
  const keepSsh = async (serverId: number) => {
    if (!onKeepSsh) return;
    setBusy(true);
    try { await onKeepSsh(serverId); } finally { setBusy(false); }
  };

  return createPortal(
    <div className="fixed inset-0 z-[9998] bg-black/60 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="held-review-title"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-[560px] max-h-[80vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl font-mono text-[12px] animate-in zoom-in-95 fade-in duration-150"
      >
        <div className="flex items-start justify-between gap-3 p-4 pb-2">
          <div className="flex items-center gap-2">
            <ShieldAlert size={14} className="text-amber-400" />
            <span id="held-review-title" className="text-[11px] font-black uppercase tracking-widest text-zinc-200">
              Waiting for your OK
            </span>
          </div>
          <button aria-label="Close" onClick={onClose} className="text-zinc-500 hover:text-white shrink-0">
            <X size={12} />
          </button>
        </div>
        <p className="px-4 pb-3 text-[12px] text-zinc-400 leading-relaxed">
          Someone you share this profile with made these changes. Until you allow one, it doesn't run on this
          device: commands aren't typed, forwards stay stopped, autostart doesn't connect, monitor commands
          don't run, and a server changed to telnet or VNC stays what it was. Allowing counts for this device only.
        </p>
        <div className="flex-1 overflow-y-auto custom-scrollbar px-4 pb-2 space-y-3">
          {held.map((s) => (
            <section key={s.id} aria-label={s.name} className="rounded-lg border border-white/10 bg-white/[0.02] p-3">
              <div className="flex items-center justify-between gap-2 mb-2">
                <div className="min-w-0">
                  <div className="text-[12.5px] font-bold text-zinc-100 truncate">{s.name}</div>
                  {s.edited_by && <div className="text-[10.5px] text-zinc-500 truncate">last changed by {s.edited_by}</div>}
                </div>
                {(s.held || []).length > 1 && (
                  <button
                    disabled={busy}
                    onClick={() => allow(s, [])}
                    className="shrink-0 px-2.5 h-7 rounded text-[10.5px] font-bold uppercase tracking-wider bg-white/[0.04] border border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white disabled:opacity-50"
                  >
                    Allow all
                  </button>
                )}
              </div>
              <ul className="space-y-2">
                {[...(s.held || [])].sort(byRank).map((item) => {
                  const d = describe(item, s);
                  return (
                    <li key={item} className="flex items-start justify-between gap-3">
                      <div className="min-w-0 flex-1">
                        <div className="text-[12px] text-zinc-200 break-words">{d.what}</div>
                        {d.detail && (
                          item === "run_on_connect" || item.startsWith("metric:") ? (
                            <pre className="mt-1 max-h-32 overflow-auto custom-scrollbar rounded bg-black/40 border border-white/5 px-2 py-1 text-[11px] text-amber-200/90 whitespace-pre-wrap break-all">{d.detail}</pre>
                          ) : (
                            <div className="mt-0.5 text-[11px] text-zinc-500">{d.detail}</div>
                          )
                        )}
                      </div>
                      <div className="shrink-0 flex flex-col items-stretch gap-1">
                        <button
                          disabled={busy}
                          onClick={() => allow(s, [item])}
                          aria-label={`Allow: ${d.what}`}
                          className="px-2.5 h-7 rounded text-[10.5px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25 disabled:opacity-50"
                        >
                          Allow
                        </button>
                        {switchOf(item) && onKeepSsh && (
                          <button
                            disabled={busy}
                            onClick={() => keepSsh(s.id)}
                            aria-label={`Keep ${s.name} an ${keptLabel(s)} server`}
                            className="px-2.5 h-7 rounded text-[10.5px] font-bold uppercase tracking-wider bg-white/[0.04] border border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white disabled:opacity-50"
                          >
                            Keep it {keptLabel(s)}
                          </button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
        <div className="flex justify-end p-4 pt-2">
          <button
            onClick={onClose}
            data-default-focus
            className="px-3 h-8 rounded text-[11px] font-bold uppercase tracking-wider bg-white/[0.04] border border-white/10 text-zinc-300 hover:bg-white/[0.08] hover:text-white"
          >
            Close
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}

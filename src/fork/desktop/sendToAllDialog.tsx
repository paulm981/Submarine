// Send to all (#218): the box for a set of commands, from Broadcast's bar.
// It says who gets them before anything goes, and who's left out.
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Send, X } from "lucide-react";
import { lineCount, prepareSend } from "../terminal/sendToAll";

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

/** Who's left out, in words: "1 paused, 2 not connected". */
export const leftOutWords = (o: { paused: number; waiting: number; offline: number }): string =>
  [
    o.paused > 0 && `${o.paused} paused`,
    o.offline > 0 && `${o.offline} not connected`,
    o.waiting > 0 && `${o.waiting} waiting for the next line`,
  ].filter(Boolean).join(", ");

export const SendToAllDialog = ({ reach, onSend, onClose }: {
  /** Who it would reach now, and who's left out; asked again as it's sent. */
  reach: () => { to: number; total: number; paused: number; waiting: number; offline: number };
  /** Send these bytes; the dialog closes after. */
  onSend: (data: number[], lines: number, cleaned: boolean) => void;
  onClose: () => void;
}) => {
  const [text, setText] = useState("");
  const [enter, setEnter] = useState(true);
  const [why, setWhy] = useState<string | null>(null);
  const r = reach();
  const lines = lineCount(text);
  const out = leftOutWords(r);
  const ready = text.trim().length > 0 && r.to > 0;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const send = () => {
    const p = prepareSend(text, enter);
    if (!p.ok) return setWhy(p.why);
    if (reach().to === 0) return setWhy("No server is taking what's typed right now.");
    onSend(p.data, p.lines, p.cleaned);
  };

  return createPortal(
    <div className="fixed inset-0 z-[80] bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Send to all"
        className="w-full max-w-xl flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <Send size={13} className="text-orange-300" /> Send to all
          </span>
          <button onClick={onClose} aria-label="Close" className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>
        <div className="p-4 flex flex-col gap-3">
          <p className="text-[11.5px] text-zinc-400 leading-relaxed">
            Typed into {r.to === r.total ? `all ${plural(r.to, "server")}` : `${r.to} of ${plural(r.total, "server")}`} on Broadcast, line by line,
            as if you typed it there.{out && ` Left out: ${out}.`}
          </p>
          <textarea
            autoFocus
            value={text}
            onChange={(e) => { setText(e.target.value); setWhy(null); }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && ready) {
                e.preventDefault();
                send();
              }
            }}
            spellCheck={false}
            rows={8}
            placeholder={"echo 'ssh-ed25519 AAAA… me@laptop' >> ~/.ssh/authorized_keys"}
            className="w-full p-3 bg-black/40 border border-white/10 rounded-lg font-mono text-[12px] text-zinc-100 outline-none focus:border-primary/50 resize-y custom-scrollbar select-text"
          />
          <label className="flex items-center gap-2 text-[11.5px] text-zinc-300 select-none">
            <input type="checkbox" className="accent-orange-400" checked={enter} onChange={(e) => setEnter(e.target.checked)} />
            Press Enter after the last line
          </label>
          {why && <p role="alert" className="text-[11.5px] text-rose-300">{why}</p>}
        </div>
        <div className="px-4 py-3 border-t border-white/5 flex items-center justify-end gap-2">
          <button onClick={onClose} className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider text-zinc-300 hover:bg-white/5">Cancel</button>
          <button
            disabled={!ready}
            onClick={send}
            title="Ctrl+Enter"
            className="h-8 px-3 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-orange-500/20 border border-orange-400/40 text-orange-200 hover:bg-orange-500/30 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Send {lines > 0 ? plural(lines, "line") : ""} to {plural(r.to, "server")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
};

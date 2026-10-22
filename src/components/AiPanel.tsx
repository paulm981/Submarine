import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bot, Check, ChevronDown, ChevronRight, Loader2, ShieldAlert, Square, X, Zap } from "lucide-react";
import { AiDone, AiPending, AiSnapshot } from "../util/ai";
import { HIDDEN_CONTROLS } from "../util/fileName";
import { WILDCARD_WARNING } from "./AiTrust";

// The AI panel (#32): one for the whole app, docked on the right and opened
// from the title bar's AI button. It holds what every AI tab does, on every
// server: who has what shared (and Stop), the requests waiting for you, what
// is running, and what ran — the exit code, all of the output, and what the
// AI was given.
//
// A request never takes the keyboard: nothing here takes focus, the buttons
// can't be reached with Tab, and only a click (never Enter or Space) allows
// or denies, so typing in a terminal can't approve anything. The two fields
// a card can show (a note with Deny, the rule for "Always allow") take the
// keyboard only when clicked, and Enter in them does nothing: they never
// approve anything by themselves.

/// A click with the mouse (or a tap), not a key: a keyboard "click" has no
/// detail count.
const byPointer = (e: React.MouseEvent) => e.detail > 0;

// What would hide or reorder text, shown plainly: the characters file names
// are checked for (HIDDEN_CONTROLS), and the other control characters but
// tab and newline. Matched by code point (the `u` flag), so a character
// outside the BMP shows as one code.
const OTHER_CONTROLS = /[\u0000-\u0008\u000B-\u001F\u007F-\u009F]/gu;
const HIDDEN_SOURCE = `${HIDDEN_CONTROLS.source}|${OTHER_CONTROLS.source}`;

/// `text` with every hidden or control character shown as its code, marked.
export const showPlainly = (text: string): ReactNode[] => {
  const re = new RegExp(HIDDEN_SOURCE, "gu");
  const out: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > last) out.push(text.slice(last, at));
    const code = m[0].codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0");
    out.push(
      <span key={at} className="px-0.5 mx-px rounded bg-amber-500/20 text-amber-300 font-bold" title="A hidden or control character">
        ⟨U+{code}⟩
      </span>,
    );
    last = at + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
};

const hasHidden = (text: string) => new RegExp(HIDDEN_SOURCE, "u").test(text);

const time = (iso: string) => {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? "" : d.toLocaleTimeString();
};

const NoFocusButton = ({ onPick, className, children, title, onDown, accept }: {
  onPick: () => void; className: string; children: ReactNode; title?: string;
  /// Told of the mouse going down on it.
  onDown?: () => void;
  /// Whether this click counts, beyond being a pointer's.
  accept?: (e: React.MouseEvent) => boolean;
}) => (
  <button
    type="button"
    tabIndex={-1}
    title={title}
    onMouseDown={(e) => { e.preventDefault(); onDown?.(); }}
    onClick={(e) => { if (byPointer(e) && (!accept || accept(e))) onPick(); }}
    onKeyDown={(e) => e.preventDefault()}
    className={className}
  >
    {children}
  </button>
);

/// How long a request's card ignores clicks after it appears or moves.
const SETTLE_MS = 500;

/// A request waiting for you. Its Allow and Deny only take a deliberate
/// click: a single click (not a double-click's second), pressed and
/// released on this card, and not within SETTLE_MS of the card appearing or
/// moving. So when a card above it goes (the AI can withdraw one) and this
/// one slides up under the pointer, a click meant for the other can't
/// decide it.
export const PendingCard = ({ p, level, decide, act, onGoToServer }: {
  p: AiPending;
  /// The server's approval level now (#115).
  level: number;
  /// Allow, or Deny with an optional note for the AI.
  decide: (allow: boolean, note?: string) => void;
  /// The card's other actions: "Always allow this command here" and "Allow
  /// everything here until this AI tab closes".
  act: (cmd: string, args: Record<string, unknown>) => void;
  onGoToServer: (sessionId: string) => void;
}) => {
  const ref = useRef<HTMLDivElement>(null);
  // Which of the card's extra parts is open: the rule to save, or the note
  // to deny with.
  const [extra, setExtra] = useState<"rule" | "note" | null>(null);
  const [rule, setRule] = useState(p.command.trim());
  const [note, setNote] = useState("");
  const settledAt = useRef(0);
  const lastTop = useRef<number | null>(null);
  const downHere = useRef(false);
  // Where it is on screen now, against where it was: moved, its clicks wait
  // SETTLE_MS again. On the screen, not in its list (offsetTop), so
  // scrolling counts too: the AI withdrawing a card above can make the
  // panel scroll this one under the pointer without it moving in the list.
  const moved = () => {
    const el = ref.current;
    if (!el) return false;
    const top = el.getBoundingClientRect().top;
    if (lastTop.current === top) return false;
    lastTop.current = top;
    settledAt.current = performance.now() + SETTLE_MS;
    downHere.current = false;
    return true;
  };
  // On every render (any change to the panel).
  useLayoutEffect(() => { moved(); });
  // And when it moves without a render: any scrolling (caught on the way
  // down, so the panel's own counts), the window resizing, text above it
  // wrapping differently.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => { moved(); };
    document.addEventListener("scroll", check, true);
    window.addEventListener("resize", check);
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(check);
    if (ro) {
      ro.observe(el);
      if (el.parentElement) ro.observe(el.parentElement);
    }
    return () => {
      document.removeEventListener("scroll", check, true);
      window.removeEventListener("resize", check);
      ro?.disconnect();
    };
  }, []);
  // Looked at again as the button goes down and as the click comes: a move
  // nothing above reported still counts.
  const accept = (e: React.MouseEvent) => {
    const ok = !moved() && e.detail === 1 && downHere.current && performance.now() >= settledAt.current;
    downHere.current = false;
    return ok;
  };
  const down = () => { downHere.current = !moved() && performance.now() >= settledAt.current; };
  const loginIsRoot = p.user === "root";
  // The fields: Enter does nothing, and no key in them reaches the app.
  const fieldKeys = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Enter") e.preventDefault();
  };
  const link = "text-[10.5px] text-zinc-400 hover:text-white underline-offset-2 hover:underline text-left";
  const field = "w-full h-7 px-2 bg-black/40 border border-white/10 rounded-md font-mono text-[11px] text-zinc-100 outline-none focus:border-primary/50 placeholder:text-zinc-600";
  return (
    <div ref={ref} className="m-2 rounded-xl border border-primary/30 bg-primary/[0.06] p-3 space-y-2">
      <div className="text-[11.5px] text-zinc-300">
        <span className="font-bold text-white">{p.tab_label}</span> wants to run on{" "}
        <NoFocusButton onPick={() => onGoToServer(p.session_id)} className="font-bold text-white hover:underline">{p.server}</NoFocusButton>
        {p.user && <> as <span className={`font-bold ${loginIsRoot ? "text-amber-300" : "text-white"}`}>{p.user}</span></>}
        {p.timeout_secs !== 60 && <span className="text-zinc-500"> · stops after {p.timeout_secs}s</span>}
      </div>
      {p.root && (
        <div className="flex items-start gap-1.5 text-[11px] text-amber-300">
          <ShieldAlert size={13} className="shrink-0 mt-px" />
          <span>
            {loginIsRoot
              ? "Runs as root: this server's tab is logged in as root."
              : "Asks for root (sudo, dzdo, su, doas or pkexec). Where those need no password, it runs as root and nothing asks first."}
          </span>
        </div>
      )}
      {hasHidden(p.command) && (
        <div className="flex items-start gap-1.5 text-[11px] text-amber-300">
          <ShieldAlert size={13} className="shrink-0 mt-px" />
          <span>It has hidden or control characters, shown below as ⟨U+…⟩.</span>
        </div>
      )}
      {p.ask_rule && (
        <div className="flex items-start gap-1.5 text-[11px] text-amber-300">
          <ShieldAlert size={13} className="shrink-0 mt-px" />
          <span>Asks because of your always-ask rule <code className="text-amber-200">{p.ask_rule}</code>.</span>
        </div>
      )}
      {/* The whole command, never cut off. */}
      <pre className="whitespace-pre-wrap break-all rounded-lg bg-black/50 border border-white/10 p-2 font-mono text-[11.5px] text-zinc-100 select-text">
        {showPlainly(p.command)}
      </pre>
      <div className="flex gap-2">
        <NoFocusButton
          onPick={() => decide(true)}
          onDown={down}
          accept={accept}
          className="flex-1 h-8 rounded-lg bg-primary/15 border border-primary/40 text-primary text-[11px] font-bold uppercase tracking-wider hover:bg-primary hover:text-zinc-950 flex items-center justify-center gap-1.5"
        >
          <Check size={13} /> Allow
        </NoFocusButton>
        <NoFocusButton
          onPick={() => decide(false)}
          onDown={down}
          accept={accept}
          className="flex-1 h-8 rounded-lg bg-zinc-900 border border-white/10 text-zinc-300 text-[11px] font-bold uppercase tracking-wider hover:border-red-400/50 hover:text-red-300 flex items-center justify-center gap-1.5"
        >
          <X size={13} /> Deny
        </NoFocusButton>
      </div>
      <div className="flex flex-col items-start gap-1">
        {p.can_allow_always && extra !== "rule" && (
          <NoFocusButton onPick={() => setExtra("rule")} className={link}>Always allow this command here…</NoFocusButton>
        )}
        {extra === "rule" && (
          <div className="w-full space-y-1.5 rounded-lg border border-white/10 bg-black/20 p-2">
            <div className="text-[10.5px] text-zinc-400">Always allow on {p.server}:</div>
            <input value={rule} onChange={(e) => setRule(e.target.value)} onKeyDown={fieldKeys} spellCheck={false} aria-label="The command to always allow" className={field} />
            <p className="text-[10.5px] text-zinc-500 leading-snug">
              Exactly this command. End it with <code>&nbsp;*</code> to allow any arguments.
              {level < 2 && " This server asks every time now: saving also sets it to “Run my allowed commands, ask for the rest”."}
            </p>
            {rule.trim().endsWith("*") && <p className="text-[10.5px] text-amber-300 leading-snug">{WILDCARD_WARNING}</p>}
            <div className="flex gap-2">
              <NoFocusButton
                onPick={() => act("ai_allow_always", { id: p.id, pattern: rule })}
                onDown={down}
                accept={accept}
                className="h-7 px-2.5 rounded-md bg-primary/15 border border-primary/40 text-primary text-[10.5px] font-bold uppercase tracking-wider hover:bg-primary hover:text-zinc-950"
              >
                Save and allow
              </NoFocusButton>
              <NoFocusButton onPick={() => setExtra(null)} className="h-7 px-2.5 rounded-md border border-white/10 text-zinc-400 text-[10.5px] font-bold uppercase tracking-wider hover:text-white">
                Cancel
              </NoFocusButton>
            </div>
          </div>
        )}
        <NoFocusButton
          onPick={() => act("ai_allow_session", { id: p.id })}
          onDown={down}
          accept={accept}
          title="Runs this, and what this AI tab asks for on this server from now on, without asking, but for your always-ask list. Ends when the AI tab closes, or the server stops being shared."
          className={`${link} flex items-center gap-1`}
        >
          <Zap size={11} className="shrink-0" /> Allow everything here until this AI tab closes
        </NoFocusButton>
        {extra !== "note" && (
          <NoFocusButton onPick={() => setExtra("note")} className={link}>Deny with a note…</NoFocusButton>
        )}
        {extra === "note" && (
          <div className="w-full space-y-1.5 rounded-lg border border-white/10 bg-black/20 p-2">
            <input
              value={note}
              onChange={(e) => setNote(e.target.value)}
              onKeyDown={fieldKeys}
              maxLength={500}
              placeholder="Why, so the AI can try another way"
              aria-label="A note for the AI"
              className={`${field} font-sans`}
            />
            <div className="flex gap-2">
              <NoFocusButton
                onPick={() => decide(false, note)}
                onDown={down}
                accept={accept}
                className="h-7 px-2.5 rounded-md bg-zinc-900 border border-white/10 text-zinc-300 text-[10.5px] font-bold uppercase tracking-wider hover:border-red-400/50 hover:text-red-300"
              >
                Deny with this note
              </NoFocusButton>
              <NoFocusButton onPick={() => setExtra(null)} className="h-7 px-2.5 rounded-md border border-white/10 text-zinc-400 text-[10.5px] font-bold uppercase tracking-wider hover:text-white">
                Cancel
              </NoFocusButton>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

/// A shared server's level, for its requests' cards.
export const levelOf = (snap: AiSnapshot, tabId: string, sessionId: string): number =>
  snap.tabs.find((t) => t.id === tabId)?.servers.find((s) => s.session_id === sessionId)?.level ?? 1;

const ranWithoutAsking = (why: string) => <span className="text-amber-300/90">ran without asking: {why}</span>;

const outcomeLabel = (d: AiDone) => {
  switch (d.outcome) {
    case "denied": return "denied by you";
    case "cancelled": return "cancelled";
    case "stopped": return "stopped";
    case "failed": return "couldn't run";
    default: return d.timed_out ? "timed out" : d.exit_code === null ? "ended" : `exit ${d.exit_code}`;
  }
};

const DoneRow = ({ d }: { d: AiDone }) => {
  const [open, setOpen] = useState(false);
  const [aiView, setAiView] = useState(false);
  // Fetched when the row is first opened (ai_output).
  const [outputs, setOutputs] = useState<{ output: string; ai_output: string } | null>(null);
  const [outputError, setOutputError] = useState<string | null>(null);
  const toggle = () => {
    if (!open && !outputs && d.outcome === "done") {
      invoke<{ output: string; ai_output: string }>("ai_output", { id: d.id }).then(setOutputs).catch((e) => setOutputError(String(e)));
    }
    setOpen((o) => !o);
  };
  const ok = d.outcome === "done" && d.exit_code === 0;
  const ran = d.outcome === "done";
  return (
    <div className="border-t border-white/5">
      <NoFocusButton
        onPick={toggle}
        className="w-full flex items-start gap-2 px-3 py-2 text-left hover:bg-white/[0.03]"
      >
        {open ? <ChevronDown size={12} className="mt-0.5 shrink-0 text-zinc-500" /> : <ChevronRight size={12} className="mt-0.5 shrink-0 text-zinc-500" />}
        <span className="flex-1 min-w-0">
          <span className="flex items-center gap-2 text-[10.5px] text-zinc-500">
            <span className="font-semibold text-zinc-300">{d.tab_label}</span>
            <span>{d.server}</span>
            <span className="ml-auto">{time(d.at)}</span>
          </span>
          <span className="block font-mono text-[11px] text-zinc-200 truncate">{showPlainly(d.command)}</span>
          <span className={`block text-[10.5px] ${ok ? "text-emerald-400" : ran ? "text-amber-300" : "text-zinc-500"}`}>
            {outcomeLabel(d)}
            {d.ran_without_asking && <> · {ranWithoutAsking(d.ran_without_asking)}</>}
            {d.hidden > 0 && <span className="text-sky-300"> · {d.hidden} hidden from the AI</span>}
            {d.cut && <span> · stopped at 1 MB</span>}
          </span>
        </span>
      </NoFocusButton>
      {open && (
        <div className="px-3 pb-3 space-y-2">
          {d.error && <p className="text-[11px] text-red-400 break-words">{d.error}</p>}
          {outputError && <p className="text-[11px] text-red-400 break-words">{outputError}</p>}
          {ran && (
            <>
              <div className="flex gap-1 text-[10px] font-bold uppercase tracking-wider">
                <NoFocusButton onPick={() => setAiView(false)} className={`px-2 py-0.5 rounded ${!aiView ? "bg-white/10 text-white" : "text-zinc-500 hover:text-zinc-300"}`}>
                  Full output
                </NoFocusButton>
                <NoFocusButton onPick={() => setAiView(true)} className={`px-2 py-0.5 rounded ${aiView ? "bg-white/10 text-white" : "text-zinc-500 hover:text-zinc-300"}`}>
                  What the AI got
                </NoFocusButton>
              </div>
              {/* Text only, never HTML. */}
              <pre className="max-h-72 overflow-auto custom-scrollbar whitespace-pre-wrap break-all rounded-lg bg-black/40 border border-white/5 p-2 font-mono text-[10.5px] text-zinc-300 select-text">
                {outputs ? ((aiView ? outputs.ai_output : outputs.output) || "(no output)") : "…"}
              </pre>
            </>
          )}
        </div>
      )}
    </div>
  );
};

export const AiPanel = ({ snap, onClose, onGoToServer }: { snap: AiSnapshot; onClose: () => void; onGoToServer: (sessionId: string) => void }) => {
  const [error, setError] = useState<string | null>(null);
  const run = (cmd: string, args: Record<string, unknown>) => {
    setError(null);
    invoke(cmd, args).catch((e) => setError(String(e)));
  };
  return (
    <aside
      aria-label="AI panel"
      className="w-[380px] shrink-0 min-h-0 flex flex-col bg-[#111114] border-l border-white/10 text-[12px]"
    >
      <div className="h-11 shrink-0 px-3 flex items-center gap-2 border-b border-white/5">
        <Bot size={15} className="text-primary" />
        <span className="font-bold text-zinc-100 tracking-wide">AI</span>
        <span className="flex-1" />
        <NoFocusButton onPick={onClose} title="Close the panel" className="w-7 h-7 flex items-center justify-center rounded hover:bg-white/10 text-zinc-400 hover:text-white">
          <X size={14} />
        </NoFocusButton>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar">
        {!snap.enabled && (
          <p className="p-4 text-zinc-500 leading-relaxed">AI tools are off. Turn them on in Settings › AI tools.</p>
        )}
        {snap.enabled && snap.tabs.length === 0 && (
          <p className="p-4 text-zinc-500 leading-relaxed">
            No AI tab is open. Start Claude Code from Home's menu (the arrow beside +), under AI tools.
          </p>
        )}

        {/* The AI tabs, what each has, and Stop. */}
        {snap.tabs.map((t) => (
          <div key={t.id} className="px-3 py-2 border-b border-white/5 flex items-start gap-2">
            <Bot size={13} className="mt-0.5 shrink-0 text-zinc-400" />
            <div className="flex-1 min-w-0">
              <div className="font-semibold text-zinc-200">{t.label}</div>
              {t.intruder_at && (
                <div className="my-1 flex items-start gap-1.5 text-[11px] text-amber-300 leading-snug">
                  <ShieldAlert size={13} className="shrink-0 mt-px" />
                  <span>
                    Another program tried to connect as this AI tab{time(t.intruder_at) && <> at {time(t.intruder_at)}</>}, and was refused.
                    Only the AI tool in that tab should connect: another program running as you may have copied its access. If you didn't expect this, close the AI tab.
                  </span>
                </div>
              )}
              <div className="text-[11px] text-zinc-500 flex flex-wrap gap-x-2">
                {t.servers.length === 0 ? "nothing shared" : (
                  <>
                    <span>shared:</span>
                    {t.servers.map((s) => (
                      <span key={s.session_id} className="inline-flex items-center gap-0.5">
                        <NoFocusButton onPick={() => onGoToServer(s.session_id)} className="text-zinc-300 hover:text-white hover:underline">{s.name}</NoFocusButton>
                        <NoFocusButton
                          onPick={() => run("ai_unshare", { sessionId: s.session_id, tabId: t.id })}
                          title={`Stop sharing ${s.name} with ${t.label}`}
                          className="text-zinc-600 hover:text-red-400"
                        >
                          <X size={10} />
                        </NoFocusButton>
                      </span>
                    ))}
                  </>
                )}
              </div>
            </div>
            <NoFocusButton
              onPick={() => run("ai_stop", { tabId: t.id })}
              title={`Stop: share nothing more with ${t.label}, and cancel what it asked for`}
              className="shrink-0 h-6 px-2 rounded border border-white/10 text-[10px] font-bold uppercase tracking-wider text-zinc-300 hover:border-red-400/50 hover:text-red-300 flex items-center gap-1"
            >
              <Square size={9} /> Stop
            </NoFocusButton>
          </div>
        ))}

        {/* Waiting for you. */}
        {snap.pending.map((p) => (
          <PendingCard
            key={p.id}
            p={p}
            level={levelOf(snap, p.tab_id, p.session_id)}
            decide={(allow, note) => run("ai_decide", { id: p.id, allow, note: note ?? null })}
            act={run}
            onGoToServer={onGoToServer}
          />
        ))}

        {/* Running now. */}
        {snap.running.map((r) => (
          <div key={r.id} className="px-3 py-2 border-t border-white/5 flex items-start gap-2">
            <Loader2 size={12} className="mt-0.5 shrink-0 animate-spin text-primary" />
            <span className="flex-1 min-w-0">
              <span className="block text-[10.5px] text-zinc-500">
                <span className="font-semibold text-zinc-300">{r.tab_label}</span> {r.server} · running
                {r.ran_without_asking && <> · {ranWithoutAsking(r.ran_without_asking)}</>}
              </span>
              <span className="block font-mono text-[11px] text-zinc-200 truncate">{showPlainly(r.command)}</span>
            </span>
          </div>
        ))}

        {snap.history.map((d) => <DoneRow key={d.id} d={d} />)}
      </div>

      {error && <p className="shrink-0 px-3 py-2 text-[11px] text-red-400 border-t border-white/5">{error}</p>}
      <p className="shrink-0 px-3 py-2 text-[10px] text-zinc-600 leading-snug border-t border-white/5">
        The AI gets output with known secrets hidden: private keys, password and token values, password hashes, access keys,
        passwords in URLs. The filter catches common shapes, not everything.
        {snap.audit_log && <> Every request is logged in {snap.audit_log}.</>}
      </p>
    </aside>
  );
};

export default AiPanel;

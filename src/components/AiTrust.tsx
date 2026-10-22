import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, ShieldCheck, Trash2, X } from "lucide-react";
import { AI_LEVELS, AiTrustTarget, closeAiTrust, levelTitle, useAiTrustTarget } from "../util/ai";

// How much the AI may run without asking (#115, src-tauri/src/ai/trust.rs):
// the level and the two lists of a server, a folder or every server. Kept on
// this device only; the AI can't change them. The editor is shown in a window
// opened from a server tab's AI marker, the AI pane, or the session tree's
// menus (AiTrustModal), and in Settings › AI tools for every server
// (AiTrustOverview), which also lists everything set.

interface RuleRow { id: number; scope: string; scope_id: number; name: string; kind: "allow" | "ask"; pattern: string }
interface ScopeView {
  own_level: number | null; inherited_level: number; inherited_from: string;
  rules: RuleRow[]; inherited_allow: number; inherited_ask: number;
}
interface TrustView { default_level: number; levels: { scope: string; scope_id: number; name: string; level: number }[]; rules: RuleRow[] }

const errText = (e: unknown) => (typeof e === "string" ? e : (e as Error)?.message || String(e));

/// Loads `load` now and whenever the AI state changes (a rule saved from a
/// request's card, say).
const useLoaded = <T,>(load: () => Promise<T>): [T | null, string | null, () => void] => {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    load().then((d) => { setData(d); setError(null); }, (e) => setError(errText(e)));
  }, [load]);
  useEffect(() => {
    reload();
    const un = listen("ai-changed", reload);
    return () => { void un.then((u) => u()); };
  }, [reload]);
  return [data, error, reload];
};

export const WILDCARD_WARNING =
  "A * allows any arguments, and some arguments can run other commands, such as find -exec or awk. Use an exact command where it will do.";

const where = (r: Pick<RuleRow, "scope" | "name">) =>
  r.scope === "default" ? "every server" : `${r.scope === "folder" ? "folder" : "server"} ${r.name || "(gone)"}`;

/// One list of rules: what's in it, Remove, and a field to add one.
const RuleList = ({ kind, rules, target, inherited, onChange }: {
  kind: "allow" | "ask"; rules: RuleRow[]; target: AiTrustTarget; inherited: number; onChange: () => void;
}) => {
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const add = () => {
    if (!draft.trim()) return;
    setError(null);
    invoke("ai_trust_add_rule", { scope: target.scope, id: target.id, kind, pattern: draft })
      .then(() => { setDraft(""); onChange(); }, (e) => setError(errText(e)));
  };
  const remove = (id: number) => {
    setError(null);
    invoke("ai_trust_remove_rule", { id }).then(onChange, (e) => setError(errText(e)));
  };
  const own = rules.filter((r) => r.kind === kind);
  return (
    <div className="space-y-1.5">
      <div className="text-[11px] font-bold uppercase tracking-wider text-zinc-400 flex items-center gap-1.5">
        {kind === "allow" ? <ShieldCheck size={12} className="text-emerald-400" /> : <ShieldAlert size={12} className="text-amber-300" />}
        {kind === "allow" ? "Always allow" : "Always ask"}
      </div>
      <p className="text-[11px] text-zinc-500 leading-snug">
        {kind === "allow"
          ? <>Runs without asking at “Run my allowed commands”. <code>uptime</code> allows exactly that; <code>systemctl status *</code> allows it with any arguments. Only one simple command matches: one with ; &amp; | $ ` &gt; &lt; or a new line always asks.</>
          : <>Always waits for your Allow, at every level. Found anywhere in a command, as whole words: <code>reboot</code> also catches <code>sudo reboot</code> and <code>uptime; reboot</code>. Write each spelling you want caught: <code>rm -rf *</code> doesn't catch <code>rm -fr</code>.</>}
      </p>
      {own.length > 0 && (
        <ul className="rounded-lg border border-white/5 divide-y divide-white/5">
          {own.map((r) => (
            <li key={r.id} className="flex items-center gap-2 px-2.5 py-1">
              <code className="flex-1 min-w-0 break-all text-[11.5px] text-zinc-200">{r.pattern}</code>
              <button onClick={() => remove(r.id)} title={`Remove ${r.pattern}`} className="shrink-0 text-zinc-500 hover:text-red-400">
                <Trash2 size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {inherited > 0 && (
        <p className="text-[11px] text-zinc-500">
          And {inherited} more from {target.scope === "server" ? "its folders and " : target.scope === "folder" ? "the folders above and " : ""}every server's list (Settings › AI tools).
        </p>
      )}
      <div className="flex gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) add(); }}
          placeholder={kind === "allow" ? "e.g. df -h" : "e.g. systemctl stop *"}
          spellCheck={false}
          className="flex-1 min-w-0 h-8 px-2.5 bg-zinc-900/60 border border-white/10 rounded-lg font-mono text-[11.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
        />
        <button onClick={add} disabled={!draft.trim()} className="shrink-0 h-8 px-3 rounded-lg bg-zinc-900 border border-white/10 text-[11px] font-bold uppercase tracking-wider text-zinc-300 hover:bg-white/5 disabled:opacity-40">
          Add
        </button>
      </div>
      {kind === "allow" && draft.trim().endsWith("*") && <p className="text-[11px] text-amber-300 leading-snug">{WILDCARD_WARNING}</p>}
      {error && <p className="text-[11px] text-red-400 break-words">{error}</p>}
    </div>
  );
};

/// A server's, a folder's or every server's level and lists.
export const TrustEditor = ({ target }: { target: AiTrustTarget }) => {
  const load = useCallback(() => invoke<ScopeView>("ai_trust_scope", { scope: target.scope, id: target.id }), [target.scope, target.id]);
  const [view, loadError, reload] = useLoaded(load);
  const [error, setError] = useState<string | null>(null);
  if (!view) return loadError ? <p className="text-[11.5px] text-red-400">{loadError}</p> : null;
  const setLevel = (level: number | null) => {
    setError(null);
    invoke("ai_trust_set_level", { scope: target.scope, id: target.id, level }).then(reload, (e) => setError(errText(e)));
  };
  const current = target.scope === "default" ? (view.own_level ?? 1) : view.own_level;
  const from = view.inherited_from ? `its folder ${view.inherited_from}` : "the default for every server";
  const option = (value: number | null, title: string, text: string) => (
    <button
      key={String(value)}
      role="radio"
      aria-checked={current === value}
      onClick={() => setLevel(value)}
      className={`w-full text-left px-3 py-2 rounded-lg border transition-all ${
        current === value ? "bg-primary/10 border-primary/40" : "bg-zinc-900/60 border-white/5 hover:bg-white/5"
      }`}
    >
      <div className={`text-[12px] font-semibold ${current === value ? "text-primary" : "text-zinc-200"}`}>{title}</div>
      <div className="text-[11px] text-zinc-500 leading-snug mt-0.5">{text}</div>
    </button>
  );
  return (
    <div className="space-y-4">
      <div role="radiogroup" aria-label="How much the AI may run without asking" className="space-y-1.5">
        {target.scope !== "default" && option(null, `Same as ${from}`, `Now: ${levelTitle(view.inherited_level)}.`)}
        {AI_LEVELS.map((l) => option(l.level, l.title, l.text))}
      </div>
      {error && <p className="text-[11px] text-red-400 break-words">{error}</p>}
      <RuleList kind="allow" rules={view.rules} target={target} inherited={view.inherited_allow} onChange={reload} />
      <RuleList kind="ask" rules={view.rules} target={target} inherited={view.inherited_ask} onChange={reload} />
      <p className="text-[11px] text-zinc-500 leading-snug">
        Kept on this computer only, never synced, and only you can change it: the AI can't.
      </p>
    </div>
  );
};

/// The editor in a window, for whatever openAiTrust was given.
export const AiTrustModal = () => {
  const target = useAiTrustTarget();
  // Nothing carries over to the next profile.
  useEffect(() => () => closeAiTrust(), []);
  useEffect(() => {
    if (!target) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") closeAiTrust(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [target]);
  if (!target) return null;
  const title = target.scope === "default" ? "AI approval for every server" : `AI approval: ${target.name}`;
  return (
    <div className="fixed inset-0 z-[10000] bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={closeAiTrust}>
      <div role="dialog" aria-modal="true" aria-label={title} className="w-full max-w-lg max-h-[85vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between gap-2">
          <span className="text-[12px] font-bold tracking-wide text-white truncate">{title}</span>
          <button onClick={closeAiTrust} aria-label="Close" className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar p-4">
          <p className="text-[11.5px] text-zinc-400 leading-snug mb-3">
            How much an AI tab may run on {target.scope === "server" ? "this server" : target.scope === "folder" ? "the servers in this folder" : "every server"} without asking you.
            {target.scope === "folder" && " A server's own setting wins over its folder's."}
          </p>
          <TrustEditor key={`${target.scope}-${target.id}`} target={target} />
        </div>
      </div>
    </div>
  );
};

/// Settings › AI tools: every server's default and lists, and everything
/// set on servers and folders, each with Remove.
export const AiTrustOverview = () => {
  const load = useCallback(() => invoke<TrustView>("ai_trust_view"), []);
  const [view, loadError, reload] = useLoaded(load);
  const [error, setError] = useState<string | null>(null);
  const run = (cmd: string, args: Record<string, unknown>) => {
    setError(null);
    invoke(cmd, args).then(reload, (e) => setError(errText(e)));
  };
  const others = view?.rules.filter((r) => r.scope !== "default") ?? [];
  return (
    <div className="space-y-4">
      <div>
        <div className="text-[12.5px] font-semibold text-zinc-100">For every server</div>
        <p className="text-[11.5px] text-zinc-500 leading-snug mt-0.5 mb-2">
          A server follows its own setting, then its folder's, then this. Set a server or a folder from its right-click menu in the session tree.
        </p>
        <TrustEditor target={{ scope: "default", id: 0, name: "every server" }} />
      </div>
      {loadError && <p className="text-[11px] text-red-400">{loadError}</p>}
      {view && (
        <div className="space-y-1.5">
          <div className="text-[12.5px] font-semibold text-zinc-100">Servers and folders that run commands without asking</div>
          {view.levels.length === 0 ? (
            <p className="text-[11.5px] text-zinc-500">None: they all follow the setting for every server.</p>
          ) : (
            <ul className="rounded-lg border border-white/5 divide-y divide-white/5">
              {view.levels.map((l) => (
                <li key={`${l.scope}-${l.scope_id}`} className="flex items-center gap-2 px-2.5 py-1.5 text-[11.5px]">
                  <span className="flex-1 min-w-0">
                    <span className="text-zinc-200">{where(l)}</span>
                    <span className="text-zinc-500"> · {levelTitle(l.level)}</span>
                  </span>
                  <button
                    onClick={() => run("ai_trust_set_level", { scope: l.scope, id: l.scope_id, level: null })}
                    title={`Remove: ${where(l)} follows ${l.scope === "server" ? "its folder" : "the folder above it"} or the setting for every server`}
                    className="shrink-0 text-zinc-500 hover:text-red-400"
                  >
                    <Trash2 size={12} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {view && (
        <div className="space-y-1.5">
          <div className="text-[12.5px] font-semibold text-zinc-100">Rules on servers and folders</div>
          {others.length === 0 ? (
            <p className="text-[11.5px] text-zinc-500">None yet. “Always allow this command here” on a request adds one.</p>
          ) : (
            <ul className="rounded-lg border border-white/5 divide-y divide-white/5">
              {others.map((r) => (
                <li key={r.id} className="flex items-center gap-2 px-2.5 py-1.5 text-[11.5px]">
                  <span className="flex-1 min-w-0">
                    <span className="text-zinc-500">{where(r)} · {r.kind === "allow" ? "always allow" : "always ask"} </span>
                    <code className="break-all text-zinc-200">{r.pattern}</code>
                  </span>
                  <button onClick={() => run("ai_trust_remove_rule", { id: r.id })} title={`Remove ${r.pattern}`} className="shrink-0 text-zinc-500 hover:text-red-400">
                    <Trash2 size={12} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {error && <p className="text-[11px] text-red-400 break-words">{error}</p>}
    </div>
  );
};

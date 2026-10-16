import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronUp, Pencil, Plus, Trash2 } from "lucide-react";
import {
  DEFAULT_HIGHLIGHT,
  HIGHLIGHT_COLORS,
  HIGHLIGHT_MINE,
  HIGHLIGHT_NONE,
  HIGHLIGHT_SETS,
  buildMatcher,
  highlightSetById,
  normalizeRules,
  ruleProblem,
  type CustomRule,
  type HighlightColor,
  type RuleKind,
} from "../ui/highlight";
import { DEFAULT_SCHEME, ansiOf, schemeById } from "../ui/terminalThemes";
import { fontFamilyCss } from "../util/terminalFont";

// Settings › Terminal › Highlighting: which set this device uses, a preview
// in the current colors, and the user's own rules (synced with the profile).

// What each set's preview shows. Made-up hosts and addresses.
const SAMPLES: Record<string, string> = {
  standard: [
    "Last login: Fri Oct 16 01:02:03 2026 from 10.20.30.40",
    "[  OK  ] Started nginx.service - web server.",
    "2026-10-16 01:02:05 INFO  connected to db at 10.20.30.41:5432",
    "2026-10-16 01:02:07 WARN  disk /var is 91% full, retrying cleanup",
    "2026-10-16 01:02:09 ERROR request failed: connection refused",
    "eth0: link/ether 00:1a:2b:3c:4d:5e  inet6 fe80::1/64",
    "Docs at https://example.com/help. Build completed successfully.",
  ].join("\n"),
  shell: [
    "#!/bin/bash",
    "# rotate the logs, keep seven",
    "for f in /var/log/app/*.log; do",
    '  if [ -s "$f" ]; then gzip "$f" || echo "failed: $f"; fi',
    "done",
    "echo 'done' && exit 0",
  ].join("\n"),
  network: [
    "interface GigabitEthernet0/1",
    " description uplink",
    " switchport mode trunk",
    " no shutdown",
    "!",
    "Gi0/1      10.0.0.1     YES manual up                    up",
    "Gi0/2      unassigned   YES unset  administratively down down",
    "access-list 10 permit 10.0.0.0 0.0.0.255",
  ].join("\n"),
  sql: [
    "SELECT name, created_at FROM users WHERE id = 42 AND status = 'active';",
    "-- yesterday's sign-ups",
    "select region, count(*) from orders where created_at > '2026-10-15' group by region;",
    'ERROR:  relation "order" does not exist',
  ].join("\n"),
};

const KINDS: { id: RuleKind; name: string }[] = [
  { id: "word", name: "Whole word" },
  { id: "text", name: "Anywhere" },
  { id: "regex", name: "Regular expression" },
];

const blankRule = (): CustomRule => ({
  pattern: "",
  kind: "word",
  matchCase: false,
  color: "bright-magenta",
  bold: true,
  underline: false,
  enabled: true,
});

const Switch = ({ on, label, onToggle }: { on: boolean; label: string; onToggle: () => void }) => (
  <button
    type="button"
    role="switch"
    aria-checked={on}
    aria-label={label}
    onClick={onToggle}
    className={`relative shrink-0 w-9 h-5 rounded-full transition-colors ${on ? "bg-primary" : "bg-zinc-700"}`}
  >
    <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white transition-all ${on ? "left-[18px]" : "left-0.5"}`} />
  </button>
);

/** `text` as a terminal would show it with this set and these rules. */
function Preview({ text, setId, rules, scheme, fontFamily }: { text: string; setId: string; rules: CustomRule[]; scheme: string; fontFamily: string }) {
  const s = schemeById(scheme || DEFAULT_SCHEME);
  const ansi = ansiOf(s);
  const fg = s.theme.foreground || "#e4e4e7";
  const matcher = useMemo(() => buildMatcher(setId, rules), [setId, rules]);
  const lines = text.split("\n").map((line, i) => {
    const spans = matcher?.find(line) ?? [];
    const out: ReactNode[] = [];
    let at = 0;
    for (const [start, end, style] of spans) {
      if (start > at) out.push(line.slice(at, start));
      const color = HIGHLIGHT_COLORS.find((c) => c.id === style.color);
      out.push(
        <span
          key={start}
          style={{
            color: color ? ansi[color.ansi] : undefined,
            fontWeight: style.bold ? 700 : undefined,
            textDecoration: style.underline ? "underline" : undefined,
          }}
        >
          {line.slice(start, end)}
        </span>,
      );
      at = end;
    }
    out.push(line.slice(at));
    return <div key={i}>{out.length ? out : " "}</div>;
  });
  return (
    <div className="rounded-xl border border-white/5 px-3 py-2.5 overflow-x-auto custom-scrollbar" style={{ background: s.theme.background }}>
      <pre className="m-0 whitespace-pre leading-snug text-[12px]" style={{ color: fg, fontFamily: fontFamilyCss(fontFamily) }}>
        {lines}
      </pre>
    </div>
  );
}

function RuleEditor({
  initial,
  scheme,
  onSave,
  onCancel,
  onDraft,
}: {
  initial: CustomRule;
  scheme: string;
  onSave: (r: CustomRule) => Promise<void>;
  onCancel: () => void;
  onDraft: (r: CustomRule) => void;
}) {
  const [rule, setRule] = useState<CustomRule>(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const ansi = ansiOf(schemeById(scheme || DEFAULT_SCHEME));
  const problem = useMemo(() => (rule.pattern ? ruleProblem(rule) : null), [rule]);
  const set = (patch: Partial<CustomRule>) => {
    const next = { ...rule, ...patch };
    setRule(next);
    onDraft(next);
  };
  const save = async () => {
    if (ruleProblem(rule)) return;
    setSaving(true);
    setError("");
    try {
      await onSave(rule);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };
  return (
    <div className="rounded-xl border border-primary/30 bg-black/30 p-3 space-y-3">
      <div className="flex gap-2">
        <input
          autoFocus
          value={rule.pattern}
          onChange={(e) => set({ pattern: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") onCancel();
          }}
          placeholder={rule.kind === "regex" ? "e.g. ERR-\\d+" : "e.g. deploy"}
          spellCheck={false}
          aria-label="Text to highlight"
          className="flex-1 min-w-0 h-8 bg-black border border-white/10 rounded-lg px-2.5 text-[12px] font-mono text-white focus:border-primary/50 outline-none"
        />
        <select
          value={rule.kind}
          onChange={(e) => set({ kind: e.target.value as RuleKind })}
          aria-label="How it matches"
          className="h-8 bg-black border border-white/10 rounded-lg px-2 text-[12px] text-white outline-none focus:border-primary/50"
        >
          {KINDS.map((k) => <option key={k.id} value={k.id}>{k.name}</option>)}
        </select>
      </div>
      <div className="flex flex-wrap items-center gap-1.5" role="radiogroup" aria-label="Color">
        {HIGHLIGHT_COLORS.map((c) => (
          <button
            key={c.id}
            type="button"
            role="radio"
            aria-checked={rule.color === c.id}
            aria-label={c.name}
            title={c.name}
            onClick={() => set({ color: c.id as HighlightColor })}
            className={`w-5 h-5 rounded-full border-2 transition-transform ${rule.color === c.id ? "border-white scale-110" : "border-transparent hover:scale-110"}`}
            style={{ background: ansi[c.ansi] }}
          />
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-[12px] text-zinc-300">
        <label className="flex items-center gap-1.5 cursor-pointer">
          <input type="checkbox" checked={rule.bold} onChange={(e) => set({ bold: e.target.checked })} className="accent-primary" /> Bold
        </label>
        <label className="flex items-center gap-1.5 cursor-pointer">
          <input type="checkbox" checked={rule.underline} onChange={(e) => set({ underline: e.target.checked })} className="accent-primary" /> Underline
        </label>
        <label className="flex items-center gap-1.5 cursor-pointer">
          <input type="checkbox" checked={rule.matchCase} onChange={(e) => set({ matchCase: e.target.checked })} className="accent-primary" /> Match case
        </label>
      </div>
      {(problem || error) && <p className="text-[11.5px] text-amber-400/90">{problem || error}</p>}
      <div className="flex justify-end gap-2">
        <button type="button" onClick={onCancel} className="h-8 px-3 rounded-lg text-[12px] font-semibold text-zinc-400 hover:text-white">
          Cancel
        </button>
        <button
          type="button"
          onClick={() => void save()}
          disabled={saving || !rule.pattern.trim() || !!problem}
          className="h-8 px-3 rounded-lg text-[12px] font-bold bg-primary text-black disabled:opacity-40"
        >
          {initial.id ? "Save" : "Add"}
        </button>
      </div>
    </div>
  );
}

export function HighlightSettings({ settings, setSettings, onRulesChanged }: any) {
  const setId: string = settings.terminalHighlight || DEFAULT_HIGHLIGHT;
  const current = highlightSetById(setId);
  const [rules, setRules] = useState<CustomRule[]>([]);
  const [loadError, setLoadError] = useState("");
  // The rule being edited (a new one has no id), and how it looks so far:
  // the preview shows it as it's typed.
  const [editing, setEditing] = useState<CustomRule | null>(null);
  const [draft, setDraft] = useState<CustomRule | null>(null);
  const [tryText, setTryText] = useState("");

  const load = async () => {
    try {
      setRules(normalizeRules(await invoke("highlight_rules_list")));
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    }
  };
  useEffect(() => {
    void load();
  }, []);

  const changed = async () => {
    await load();
    onRulesChanged?.();
  };
  const saveRule = async (r: CustomRule) => {
    await invoke("highlight_rule_save", { rule: r });
    setEditing(null);
    setDraft(null);
    await changed();
  };
  const toggle = async (r: CustomRule) => {
    try {
      await invoke("highlight_rule_save", { rule: { ...r, enabled: !r.enabled } });
    } finally {
      await changed();
    }
  };
  const remove = async (r: CustomRule) => {
    try {
      await invoke("highlight_rule_delete", { id: r.id });
    } finally {
      await changed();
    }
  };
  const move = async (i: number, by: number) => {
    const ids = rules.map((r) => r.id!);
    const j = i + by;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    try {
      await invoke("highlight_rules_reorder", { ids });
    } finally {
      await changed();
    }
  };

  // The preview's rules: the saved ones, with the one being edited as it is.
  const previewRules = useMemo(() => {
    if (!draft) return rules;
    if (draft.id) return rules.map((r) => (r.id === draft.id ? draft : r));
    return [draft, ...rules];
  }, [rules, draft]);
  const previewSet = setId === HIGHLIGHT_NONE && draft ? HIGHLIGHT_MINE : setId;
  const sample = tryText || SAMPLES[setId] || SAMPLES.standard;
  const ansi = ansiOf(schemeById(settings.terminalScheme || DEFAULT_SCHEME));

  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-5 shadow-xl">
      <div className="space-y-2">
        <div className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Highlighting</div>
        <div role="radiogroup" aria-label="Highlighting" className="grid grid-cols-2 gap-1.5">
          {HIGHLIGHT_SETS.map((s) => (
            <button
              key={s.id}
              type="button"
              role="radio"
              aria-checked={s.id === setId}
              onClick={() => setSettings((st: any) => ({ ...st, terminalHighlight: s.id }))}
              className={`text-left px-3 py-2 rounded-lg border text-[12px] font-semibold transition-colors ${
                s.id === setId ? "border-primary/60 bg-primary/10 text-white" : "border-white/5 bg-black/30 text-zinc-400 hover:text-white hover:border-white/15"
              }`}
            >
              {s.name}
            </button>
          ))}
        </div>
        <p className="text-[11.5px] text-zinc-500 leading-relaxed">
          {current.description} A server can have its own set (its edit panel). Only what you see changes: what's
          copied, typed or sent is untouched, and full-screen programs like vim, less and top are left alone, as is
          text a program colored itself.
        </p>
      </div>

      <div className="space-y-2">
        <Preview text={sample} setId={previewSet} rules={previewRules} scheme={settings.terminalScheme} fontFamily={settings.terminalFontFamily || ""} />
        <textarea
          value={tryText}
          onChange={(e) => setTryText(e.target.value)}
          rows={2}
          spellCheck={false}
          placeholder="Paste some of your own output here to see how it looks"
          aria-label="Text to try"
          className="w-full bg-black border border-white/10 rounded-lg px-2.5 py-2 text-[12px] font-mono text-zinc-200 focus:border-primary/50 outline-none resize-y"
        />
      </div>

      <div className="space-y-2 pt-4 border-t border-white/5">
        <div className="flex justify-between items-center gap-2">
          <div className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">My rules</div>
          {!editing && (
            <button
              type="button"
              onClick={() => {
                const r = blankRule();
                setEditing(r);
                setDraft(null);
              }}
              className="flex items-center gap-1 text-[11px] font-semibold text-zinc-400 hover:text-white transition-colors"
            >
              <Plus size={13} /> Add a rule
            </button>
          )}
        </div>
        {editing && !editing.id && (
          <RuleEditor
            initial={editing}
            scheme={settings.terminalScheme}
            onSave={saveRule}
            onCancel={() => { setEditing(null); setDraft(null); }}
            onDraft={setDraft}
          />
        )}
        {loadError && <p className="text-[11.5px] text-amber-400/90">Couldn't read your rules: {loadError}</p>}
        {rules.length === 0 && !editing && (
          <p className="text-[11.5px] text-zinc-500">
            None yet. Add a word or pattern to show in a color of your choice, e.g. your app's name or an error code.
          </p>
        )}
        <ul className="space-y-1.5">
          {rules.map((r, i) =>
            editing && editing.id === r.id ? (
              <li key={r.id}>
                <RuleEditor
                  initial={editing}
                  scheme={settings.terminalScheme}
                  onSave={saveRule}
                  onCancel={() => { setEditing(null); setDraft(null); }}
                  onDraft={setDraft}
                />
              </li>
            ) : (
              <li key={r.id} className={`flex items-center gap-2 rounded-lg bg-black/30 border border-white/5 px-2.5 py-1.5 ${r.enabled ? "" : "opacity-50"}`}>
                <Switch on={r.enabled} label={`Use the rule ${r.pattern}`} onToggle={() => void toggle(r)} />
                <span className="flex-1 min-w-0 flex flex-col leading-tight">
                  <span
                    className="truncate font-mono text-[12px]"
                    style={{
                      color: ansi[HIGHLIGHT_COLORS.find((c) => c.id === r.color)?.ansi ?? 1],
                      fontWeight: r.bold ? 700 : undefined,
                      textDecoration: r.underline ? "underline" : undefined,
                    }}
                    title={r.pattern}
                  >
                    {r.pattern}
                  </span>
                  <span className="text-[10.5px] text-zinc-500">
                    {KINDS.find((k) => k.id === r.kind)?.name}
                    {r.matchCase ? ", match case" : ""}
                  </span>
                </span>
                <button type="button" aria-label="Move up" title="Move up" disabled={i === 0} onClick={() => void move(i, -1)} className="p-1 text-zinc-500 hover:text-white disabled:opacity-30">
                  <ChevronUp size={14} />
                </button>
                <button type="button" aria-label="Move down" title="Move down" disabled={i === rules.length - 1} onClick={() => void move(i, 1)} className="p-1 text-zinc-500 hover:text-white disabled:opacity-30">
                  <ChevronDown size={14} />
                </button>
                <button type="button" aria-label="Edit" title="Edit" onClick={() => { setEditing(r); setDraft(null); }} className="p-1 text-zinc-500 hover:text-white">
                  <Pencil size={13} />
                </button>
                <button type="button" aria-label="Delete" title="Delete" onClick={() => void remove(r)} className="p-1 text-zinc-500 hover:text-red-400">
                  <Trash2 size={13} />
                </button>
              </li>
            ),
          )}
        </ul>
        <p className="text-[11.5px] text-zinc-500 leading-relaxed">
          Your rules sync with your profile and apply with every set but None, ahead of it: where one of yours and the
          set's match the same text, yours wins, and the higher of two of yours.
        </p>
      </div>
    </div>
  );
}

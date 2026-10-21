import { useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ExternalLink } from "lucide-react";
import {
  CUSTOM_SCHEME, DEFAULT_SCHEME, SCHEMES, ansiOf, customSchemeFrom, schemeById, type TerminalScheme,
} from "../ui/terminalThemes";
import { fontFamilyCss } from "../util/terminalFont";
import { parseWindowsTerminalScheme, schemeToWindowsTerminalJson } from "../util/terminalTheme";

// Settings › Terminal › Colors, and the small swatch the server edit panel
// shows beside its own scheme picker.

/** The scheme's background with its 8 normal colors on it. */
export function SchemeSwatch({ scheme, className = "" }: { scheme: TerminalScheme; className?: string }) {
  const ansi = ansiOf(scheme);
  return (
    <span
      className={`inline-flex items-center gap-[3px] rounded-md px-1.5 py-1 border border-white/10 ${className}`}
      style={{ background: scheme.theme.background }}
      aria-hidden
    >
      <span className="text-[10px] font-mono font-bold leading-none mr-0.5" style={{ color: scheme.theme.foreground }}>Aa</span>
      {ansi.slice(1, 7).map((c, i) => (
        <span key={i} className="w-2 h-2 rounded-full" style={{ background: c }} />
      ))}
    </span>
  );
}

/** What a shell looks like in this scheme: prompt, `ls` colors, git, an error. */
export function SchemePreview({ scheme, fontFamily, fontSize }: { scheme: TerminalScheme; fontFamily: string; fontSize: number }) {
  const a = ansiOf(scheme);
  const fg = scheme.theme.foreground || "#e4e4e7";
  const C = ({ c, b, children }: { c: string; b?: boolean; children: ReactNode }) => (
    <span style={{ color: c, fontWeight: b ? 700 : undefined }}>{children}</span>
  );
  const prompt = (
    <><C c={a[10]} b>me@web</C><C c={fg}>:</C><C c={a[12]} b>~</C><C c={fg}>$ </C></>
  );
  return (
    <div
      className="rounded-xl border border-white/5 px-3 py-2.5 overflow-x-auto custom-scrollbar"
      style={{ background: scheme.theme.background }}
    >
      <pre className="m-0 whitespace-pre leading-snug" style={{ color: fg, fontFamily: fontFamilyCss(fontFamily), fontSize }}>
        {prompt}ls{"\n"}
        <C c={a[12]} b>logs</C>  <C c={a[12]} b>src</C>  <C c={a[10]} b>run.sh</C>  a.txt{"\n"}
        {prompt}git status -s{"\n"}
        {" "}<C c={a[1]}>M</C> main.rs{"\n"}
        <C c={a[1]}>??</C> notes.txt{"\n"}
        <C c={a[10]}>●</C> nginx: <C c={a[10]} b>active</C> <C c={a[8]}>(2 min)</C>{"\n"}
        <C c={a[3]}>warning:</C> disk 91% full{"\n"}
        <C c={a[9]} b>error:</C> permission denied{"\n"}
        {prompt}<span style={{ background: scheme.theme.cursor, color: scheme.theme.background }}> </span>
      </pre>
    </div>
  );
}

const Switch = ({ on, label, onToggle }: { on: boolean; label: string; onToggle: () => void }) => (
  <button
    role="switch"
    aria-checked={on}
    aria-label={label}
    onClick={onToggle}
    className={`relative shrink-0 w-11 h-6 rounded-full transition-colors ${on ? "bg-primary" : "bg-zinc-700"}`}
  >
    <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${on ? "left-[22px]" : "left-0.5"}`} />
  </button>
);

export function TerminalColorSettings({ settings, setSettings }: any) {
  // This device's own scheme, pasted as Windows Terminal JSON (the original
  // project's #183). Read from the settings, not storage, so the preview
  // follows a new one at once.
  const custom = useMemo(() => customSchemeFrom(settings.terminalCustomScheme), [settings.terminalCustomScheme]);
  const schemes = custom ? [...SCHEMES, custom] : SCHEMES;
  const current = settings.terminalScheme === CUSTOM_SCHEME && custom
    ? custom
    : schemeById(settings.terminalScheme === CUSTOM_SCHEME ? DEFAULT_SCHEME : settings.terminalScheme || DEFAULT_SCHEME);
  const pick = (id: string) => setSettings((s: any) => ({ ...s, terminalScheme: id }));
  const gpu = settings.terminalGpu !== false;
  const contrast = settings.terminalMinContrast !== false;
  const boldBright = settings.terminalBoldBright !== false;
  const credit = current.credit;
  const [importing, setImporting] = useState(false);
  const [pasted, setPasted] = useState("");
  const [importError, setImportError] = useState<string | null>(null);
  const useImported = () => {
    try {
      if (pasted.length > 20000) throw new Error("That's too long for one color scheme.");
      const s = parseWindowsTerminalScheme(pasted);
      setSettings((prev: any) => ({ ...prev, terminalCustomScheme: schemeToWindowsTerminalJson(s), terminalScheme: CUSTOM_SCHEME }));
      setImporting(false);
      setPasted("");
      setImportError(null);
    } catch (e: any) {
      setImportError(e?.message ?? String(e));
    }
  };
  const removeCustom = () =>
    setSettings((prev: any) => ({
      ...prev,
      terminalCustomScheme: "",
      terminalScheme: prev.terminalScheme === CUSTOM_SCHEME ? DEFAULT_SCHEME : prev.terminalScheme,
    }));
  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-5 shadow-xl">
      <div className="space-y-2">
        <div className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Colors</div>
        <div role="radiogroup" aria-label="Terminal colors" className="grid grid-cols-2 gap-1.5">
          {schemes.map((s) => {
            const on = s.id === current.id;
            // Each card in its own colors: name in the foreground, the six
            // main colors below it, on the background.
            return (
              <button
                key={s.id}
                role="radio"
                aria-checked={on}
                onClick={() => pick(s.id)}
                style={{ background: s.theme.background, color: s.theme.foreground }}
                className={`relative flex flex-col items-start gap-1.5 px-2.5 py-2 rounded-lg border text-left transition-all ${
                  on ? "border-primary ring-1 ring-primary" : "border-white/10 hover:border-white/30"
                }`}
              >
                <span className={`w-full truncate text-[11.5px] font-semibold ${on ? "pr-4" : ""}`}>{s.name}</span>
                <span className="flex gap-1" aria-hidden>
                  {ansiOf(s).slice(1, 7).map((c, i) => <span key={i} className="w-2 h-2 rounded-full" style={{ background: c }} />)}
                </span>
                {on && (
                  <span className="absolute top-1.5 right-1.5 w-4 h-4 rounded-full bg-primary flex items-center justify-center">
                    <Check size={10} strokeWidth={3} className="text-zinc-950" />
                  </span>
                )}
              </button>
            );
          })}
        </div>
        <SchemePreview
          scheme={current}
          fontFamily={settings.terminalFontFamily || ""}
          fontSize={Math.min(settings.terminalFontSize || 14, 13)}
        />
        <p className="text-[11.5px] text-zinc-500 leading-relaxed">
          {credit ? (
            <>
              {current.name} by {credit.by} · {credit.license}{" "}
              <button
                onClick={() => invoke("open_external_url", { url: credit.url }).catch(() => {})}
                className="inline-flex items-center gap-0.5 text-zinc-400 hover:text-white underline-offset-2 hover:underline"
              >
                project <ExternalLink size={10} />
              </button>
              {". "}
            </>
          ) : current.id === CUSTOM_SCHEME ? (
            "Your own colors, on this computer only. "
          ) : (
            "Submarine's own colors. "
          )}
          A server can have its own colors too, in its edit panel — say, a light scheme for production.
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => { setImporting((v) => !v); setImportError(null); }}
            aria-expanded={importing}
            className="px-3 py-1.5 rounded-lg border border-white/10 text-[12px] text-zinc-200 hover:border-primary/40 hover:text-white"
          >
            {custom ? "Replace my scheme…" : "Use my own scheme…"}
          </button>
          {custom && (
            <button
              type="button"
              onClick={removeCustom}
              className="px-3 py-1.5 rounded-lg border border-white/10 text-[12px] text-zinc-400 hover:border-rose-400/40 hover:text-rose-300"
            >
              Remove {custom.name}
            </button>
          )}
        </div>
        {importing && (
          <div className="space-y-2">
            <p className="text-[11.5px] text-zinc-500 leading-relaxed">
              Paste a Windows Terminal color scheme: JSON with name, background, foreground and the 16 colors from
              black to brightWhite. Sites that collect terminal schemes offer this format. It stays on this computer.
            </p>
            <textarea
              value={pasted}
              onChange={(e) => { setPasted(e.target.value); setImportError(null); }}
              rows={6}
              spellCheck={false}
              aria-label="Windows Terminal color scheme"
              placeholder={'{\n  "name": "My scheme",\n  "background": "#1e1e2e",\n  "foreground": "#cdd6f4",\n  …\n}'}
              className="w-full bg-black border border-white/10 rounded-lg px-2 py-1.5 text-[11px] font-mono text-zinc-200 focus:border-primary/50 outline-none"
            />
            {importError && <p role="alert" className="text-[11.5px] text-rose-300">{importError}</p>}
            <button
              type="button"
              onClick={useImported}
              disabled={!pasted.trim()}
              className="px-3 py-1.5 rounded-lg bg-primary/20 border border-primary/40 text-[12px] text-white disabled:opacity-40"
            >
              Use it
            </button>
          </div>
        )}
      </div>

      <div className="flex items-center justify-between gap-4 border-t border-white/5 pt-4">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-zinc-100">Keep text readable</div>
          <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
            Text a program colors too close to the background is lightened or darkened until it reads
            (minimum contrast 4.5:1, as in VS Code).
          </p>
        </div>
        <Switch on={contrast} label="Keep text readable" onToggle={() => setSettings((s: any) => ({ ...s, terminalMinContrast: !contrast }))} />
      </div>
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-zinc-100">Bold text in bright colors</div>
          <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
            Bold text in one of the first eight colors uses its bright version, as most terminals do.
          </p>
        </div>
        <Switch on={boldBright} label="Bold text in bright colors" onToggle={() => setSettings((s: any) => ({ ...s, terminalBoldBright: !boldBright }))} />
      </div>
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-zinc-100">Draw with the GPU</div>
          <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
            Faster scrolling and big outputs. On by default on Windows. On macOS and Linux it starts off: there,
            a terminal you aren't typing in (a split pane, a Wall's tiles) can stop showing new output. Turn it off
            if text looks wrong.
          </p>
        </div>
        <Switch on={gpu} label="Draw with the GPU" onToggle={() => setSettings((s: any) => ({ ...s, terminalGpu: !gpu }))} />
      </div>
    </div>
  );
}

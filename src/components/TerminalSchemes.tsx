import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ExternalLink } from "lucide-react";
import { DEFAULT_SCHEME, SCHEMES, ansiOf, schemeById, type TerminalScheme } from "../ui/terminalThemes";
import { fontFamilyCss } from "../util/terminalFont";

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
  const current = schemeById(settings.terminalScheme || DEFAULT_SCHEME);
  const pick = (id: string) => setSettings((s: any) => ({ ...s, terminalScheme: id }));
  const gpu = settings.terminalGpu !== false;
  const contrast = settings.terminalMinContrast !== false;
  const credit = current.credit;
  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-5 shadow-xl">
      <div className="space-y-2">
        <div className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Colors</div>
        <div role="radiogroup" aria-label="Terminal colors" className="grid grid-cols-2 gap-1.5">
          {SCHEMES.map((s) => {
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
          ) : (
            "Submarine's own colors. "
          )}
          A server can have its own colors too, in its edit panel — say, a light scheme for production.
        </p>
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
          <div className="text-sm font-semibold text-zinc-100">Draw with the GPU</div>
          <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
            Faster scrolling and big outputs. Turn off if text looks wrong on this computer; terminals then draw
            the way they always did.
          </p>
        </div>
        <Switch on={gpu} label="Draw with the GPU" onToggle={() => setSettings((s: any) => ({ ...s, terminalGpu: !gpu }))} />
      </div>
    </div>
  );
}

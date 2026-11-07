// Settings › Terminal: the fork's colours and highlighting in the original
// project's Terminal Configuration card, a note on its Authentication card,
// and the Home terminal's shell.
import { useEffect, useState } from "react";
import { Home } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import CopyValue from "../CopyValue";
import { shellGroups, type LocalShells } from "../HomePane";
import { TerminalColorSettings } from "../TerminalSchemes";
import { HighlightSettings } from "../HighlightSettings";
import { DEFAULT_SCHEME } from "../../ui/terminalThemes";
import { IS_WINDOWS } from "../../util/platform";
import { CARD, LearnMore, SettingsSection } from "./parts";

/** In place of the original project's colours: the fork's schemes, then
 *  highlighting. */
export function TerminalColours({ settings, setSettings, onHighlightRulesChanged }: any) {
  return (
    <>
      <TerminalColorSettings settings={settings} setSettings={setSettings} />
      <HighlightSettings settings={settings} setSettings={setSettings} onRulesChanged={onHighlightRulesChanged} />
    </>
  );
}

/** What Maintenance's Reset to Defaults also puts back: the fork's terminal
 *  colours and drawing. */
export const TERMINAL_RESET = {
  terminalScheme: DEFAULT_SCHEME,
  terminalGpu: IS_WINDOWS,
  terminalMinContrast: true,
  terminalBoldBright: true,
} as const;

/** Under the original project's "Use settings from ~/.ssh/config": what the
 *  fork does beyond what their card says. */
export function SshConfigNote() {
  return (
    <p className="text-[11.5px] text-zinc-500 leading-relaxed px-1">
      Its IdentityFile keys and CertificateFile certificates are also tried when a server's saved password or key
      doesn't work. The host keys Submarine already trusts still come first: a changed key always gets the warning,
      and a key known_hosts marks @revoked is refused.
    </p>
  );
}

// The shell Home terminals start (src-tauri/src/local_term.rs). Kept by the
// backend, like Live Edit's editor: a program of your own is picked in the
// native dialog, never typed in here.
function HomeTerminalSettings() {
  const [info, setInfo] = useState<LocalShells | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<LocalShells>("list_local_shells").then(setInfo).catch((e) => setError(String(e)));
  }, []);
  const run = (cmd: string, args?: Record<string, unknown>) => {
    setError(null);
    invoke<LocalShells>(cmd, args).then(setInfo).catch((e) => setError(String(e)));
  };
  if (!info) return error ? <p className="text-[11.5px] text-red-400">Couldn't list the shells: {error}</p> : null;
  return (
    <div className={CARD}>
      <div>
        <p className="text-sm text-zinc-400 leading-relaxed">The shell new Home terminals start.</p>
        <LearnMore>
          Home opens a terminal on this computer, in your home folder. New ones start the shell checked here; the
          arrow beside + in Home starts any of the others.
        </LearnMore>
      </div>
      {shellGroups(info).map((g) => (
        <div key={g.name} className="space-y-1.5">
          <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">{g.name}</span>
          <div className="flex flex-col gap-1">
            {g.shells.map((s) => (
              <label key={s.id} className="flex items-center gap-2.5 px-3 min-h-9 py-1.5 rounded-xl border border-white/5 bg-zinc-900/60 hover:bg-white/[0.04] cursor-pointer">
                <input
                  type="radio"
                  name="home-shell"
                  checked={info.default_id === s.id}
                  onChange={() => run("local_shell_set_default", { id: s.id })}
                  className="accent-[var(--primary-hex)]"
                />
                <span className="min-w-0">
                  <span className="block text-[12.5px] text-zinc-200">{s.label}</span>
                  {s.detail && <span className="block text-[11px] text-zinc-500 break-all">{s.detail}</span>}
                </span>
              </label>
            ))}
            {g.notes.map((n, i) => (
              <p key={i} className="text-[11.5px] text-zinc-500 leading-relaxed px-1">{n}</p>
            ))}
          </div>
        </div>
      ))}
      {info.custom && (
        <div className="space-y-1">
          <CopyValue value={info.custom} className="max-w-full text-zinc-300">
            <span className="font-mono text-[11.5px] break-all select-text">{info.custom}</span>
          </CopyValue>
          {info.custom_missing && (
            <p className="text-[11.5px] text-amber-400/90 leading-relaxed">
              That program isn't there any more, so Home uses the shell checked above. Choose it again, or forget it.
            </p>
          )}
        </div>
      )}
      <div className="flex gap-2">
        <button
          onClick={() => run("local_shell_pick_custom")}
          className="flex-1 px-4 h-9 bg-primary/10 border border-primary/30 text-primary rounded-xl text-xs font-bold uppercase hover:bg-primary hover:text-zinc-950 transition-all"
        >
          Choose program…
        </button>
        {info.custom && (
          <button
            onClick={() => run("local_shell_clear_custom")}
            className="px-4 h-9 bg-zinc-900 border border-white/5 text-zinc-300 rounded-xl text-xs font-bold uppercase hover:bg-white/5 transition-all"
          >
            Forget it
          </button>
        )}
      </div>
      {error && <p className="text-[11.5px] text-red-400">{error}</p>}
    </div>
  );
}

export function HomeTerminalSection() {
  return (
    <SettingsSection icon={Home} title="Home Terminal">
      <HomeTerminalSettings />
    </SettingsSection>
  );
}

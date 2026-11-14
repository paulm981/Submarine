// Settings › Help: updates, troubleshooting and the open-source licenses.
import { useState } from "react";
import { ClipboardCopy, FolderOpen, LifeBuoy, RefreshCw, Scale } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import LicensesDialog from "../LicensesDialog";
import { windowSummary } from "../../util/diagnostics";
import { IS_ANDROID } from "../../util/platform";
import { CARD, LearnMore, SettingsSection, SwitchRow } from "./parts";

export function UpdatesSection({ settings, setSettings }: any) {
  return (
    <SettingsSection icon={RefreshCw} title="Updates">
      <div className={CARD}>
        <div>
          <p className="text-sm text-zinc-400 leading-relaxed">
            The profile screen says when there's a newer release of the fork.
          </p>
          <LearnMore>
            Every change to the fork is a release, numbered like v0.5.0-fork.2 and shown in the title bar. Built from
            source, it also says when the fork's main has changes to pull and rebuild.
          </LearnMore>
        </div>
        <div className="border-t border-white/5 pt-4">
          <SwitchRow
            title="Watch the original project"
            line="For whoever maintains the fork: also say when the original project has changes the fork doesn't have yet."
            more="The original project is SinaXhpm/Submarine. Its changes come in with a sync pull request (FORK.md says how)."
            on={!!settings.watchUpstream}
            onToggle={() => setSettings((s: any) => ({ ...s, watchUpstream: !s.watchUpstream }))}
          />
        </div>
      </div>
    </SettingsSection>
  );
}

// The app's log (src-tauri/src/applog.rs) and a copy of what's needed to
// look into a problem, with the servers' names, hosts and addresses replaced.
export function TroubleshootingSection() {
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const say = (ok: boolean, text: string) => {
    setStatus({ ok, text });
    window.setTimeout(() => setStatus((s) => (s?.text === text ? null : s)), 8000);
  };
  const copy = async () => {
    setBusy(true);
    try {
      const report = await invoke<string>("diagnostics_report", { ui: windowSummary() });
      await navigator.clipboard.writeText(report);
      say(true, `Copied ${report.split("\n").length} lines. Paste them to whoever is helping you.`);
    } catch (e) {
      say(false, `Couldn't copy: ${e}`);
    } finally {
      setBusy(false);
    }
  };
  const openFolder = () => {
    invoke("open_log_folder").catch((e) => say(false, String(e)));
  };
  const btn = "px-4 h-9 bg-zinc-900 border border-white/5 text-zinc-300 rounded-xl text-xs font-bold uppercase hover:bg-white/5 transition-all flex-1 flex items-center justify-center gap-2 disabled:opacity-50";
  return (
    <SettingsSection icon={LifeBuoy} title="Troubleshooting">
      <div className={CARD}>
        <div>
          <p className="text-sm text-zinc-400 leading-relaxed">
            If something goes wrong, click <span className="text-zinc-200">Copy diagnostics</span> soon after and paste
            it to whoever is helping you.
          </p>
          <LearnMore>
            Submarine keeps a log on this computer: connections, errors, and when the window freezes and which
            terminal was busy then. In the copy, server, folder and user names, host names and addresses are
            replaced with placeholders.
          </LearnMore>
        </div>
        <div className="flex flex-col sm:flex-row gap-2">
          <button onClick={copy} disabled={busy} className={btn}>
            <ClipboardCopy size={13} /> Copy diagnostics
          </button>
          {!IS_ANDROID && (
            <button onClick={openFolder} className={btn}>
              <FolderOpen size={13} /> Open log folder
            </button>
          )}
        </div>
        {status && (
          <p className={`text-[11.5px] leading-relaxed ${status.ok ? "text-emerald-300/90" : "text-rose-300"}`}>{status.text}</p>
        )}
      </div>
    </SettingsSection>
  );
}

export function LicensesSection() {
  const [open, setOpen] = useState(false);
  return (
    <SettingsSection icon={Scale} title="Open-source licenses">
      <div className={CARD}>
        <p className="text-sm text-zinc-400 leading-relaxed">
          Submarine is MIT-licensed and built on open-source packages, fonts and color schemes. See each one
          and its licence.
        </p>
        <button
          onClick={() => setOpen(true)}
          className="px-4 h-9 bg-zinc-900 border border-white/5 text-zinc-300 rounded-xl text-xs font-bold uppercase hover:bg-white/5 transition-all w-full flex items-center justify-center gap-2"
        >
          <Scale size={13} /> Open-source licenses
        </button>
      </div>
      {open && <LicensesDialog onClose={() => setOpen(false)} />}
    </SettingsSection>
  );
}

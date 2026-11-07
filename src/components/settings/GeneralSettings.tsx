// Settings › General: the fork's additions to the original project's UI
// Customization card, and Live Edit's editor.
import { useEffect, useState } from "react";
import { FileEdit, Minus, Plus } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import CopyValue from "../CopyValue";
import { DEFAULT_INFO_FONT_SIZE, MAX_INFO_FONT_SIZE, MIN_INFO_FONT_SIZE, clampInfoFontSize } from "../../util/infoFont";
import { CARD, LearnMore, SettingsSection, SwitchRow } from "./parts";

// The Info panel's text size (util/infoFont), with the − / + buttons the
// terminal's font size has. Per-device, like the rest of this card.
function InfoFontSizeSetting({ settings, setSettings }: any) {
  const size = clampInfoFontSize(settings.infoFontSize);
  const setSize = (n: number) => setSettings((s: any) => ({ ...s, infoFontSize: clampInfoFontSize(n) }));
  return (
    <div className="space-y-3 pt-6 border-t border-white/5">
      <div className="flex justify-between items-center gap-3">
        <label htmlFor="info-font-size" className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Info Panel Text Size (px)</label>
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            aria-label="Smaller Info panel text"
            onClick={() => setSize(size - 1)}
            disabled={size <= MIN_INFO_FONT_SIZE}
            className="w-8 h-8 flex items-center justify-center bg-black border border-white/10 rounded-lg text-zinc-300 hover:text-white hover:border-primary/40 disabled:opacity-30 transition-colors"
          >
            <Minus size={14} />
          </button>
          <output
            id="info-font-size"
            aria-live="polite"
            className="w-14 h-8 flex items-center justify-center bg-black border border-white/10 rounded-lg text-[12px] font-bold text-white"
          >
            {size}
          </output>
          <button
            type="button"
            aria-label="Larger Info panel text"
            onClick={() => setSize(size + 1)}
            disabled={size >= MAX_INFO_FONT_SIZE}
            className="w-8 h-8 flex items-center justify-center bg-black border border-white/10 rounded-lg text-zinc-300 hover:text-white hover:border-primary/40 disabled:opacity-30 transition-colors"
          >
            <Plus size={14} />
          </button>
        </div>
      </div>
      <p className="text-[11.5px] text-zinc-500 leading-relaxed">
        The text in a server's Info panel, from {MIN_INFO_FONT_SIZE} to {MAX_INFO_FONT_SIZE}. Its smaller labels and icons grow with it.
        {size !== DEFAULT_INFO_FONT_SIZE && (
          <>
            {" "}
            <button type="button" onClick={() => setSize(DEFAULT_INFO_FONT_SIZE)} className="font-semibold text-zinc-400 hover:text-white">
              Back to {DEFAULT_INFO_FONT_SIZE}
            </button>
          </>
        )}
      </p>
    </div>
  );
}

/** Inside the original project's UI Customization card, after their
 *  colours: the Info panel's text size and compact tabs. */
export function UiCustomizationExtras({ settings, setSettings }: any) {
  return (
    <>
      <InfoFontSizeSetting settings={settings} setSettings={setSettings} />
      <div className="pt-6 border-t border-white/5">
        <SwitchRow
          title="Compact tabs"
          line="Flat tabs that touch, so more fit across. Off, tabs are the rounded pills."
          more="The tab in front has a thin line under it. Long names are cut short: hover for the whole name."
          on={!!settings.compactTabs}
          onToggle={() => setSettings((s: any) => ({ ...s, compactTabs: !s.compactTabs }))}
        />
      </div>
    </>
  );
}

// The editor Live Edit opens remote files in (src-tauri/src/local_open.rs).
interface LiveEditEditor {
  chosen: string | null;
  default_label: string;
}

// Kept by the backend, not in local storage: the editor is started with files
// from servers, so it can only be set through the native file dialog.
function LiveEditSettings() {
  const [editor, setEditor] = useState<LiveEditEditor | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<LiveEditEditor>("live_edit_editor_get").then(setEditor).catch(() => setEditor(null));
  }, []);
  const run = (cmd: string) => {
    setError(null);
    invoke<LiveEditEditor>(cmd).then(setEditor).catch((e) => setError(String(e)));
  };
  if (!editor) return null;
  return (
    <div className={CARD}>
      <div>
        <p className="text-sm text-zinc-400 leading-relaxed">
          Double-click a remote file to edit a copy in this editor. Each save uploads it.
        </p>
        <LearnMore>
          It never runs the file: a program is refused. With no editor chosen, it's Notepad on Windows, the default
          text editor on macOS, and on Linux the text editor set for plain text.
        </LearnMore>
      </div>
      <div className="space-y-1.5">
        <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Editor</span>
        {editor.chosen ? (
          <CopyValue value={editor.chosen} className="max-w-full text-zinc-300">
            <span className="font-mono text-[11.5px] break-all select-text">{editor.chosen}</span>
          </CopyValue>
        ) : (
          <div className="text-[12.5px] text-zinc-300">{editor.default_label} (default)</div>
        )}
      </div>
      <div className="flex gap-2">
        <button
          onClick={() => run("live_edit_editor_pick")}
          className="flex-1 px-4 h-9 bg-primary/10 border border-primary/30 text-primary rounded-xl text-xs font-bold uppercase hover:bg-primary hover:text-zinc-950 transition-all"
        >
          Choose editor…
        </button>
        {editor.chosen && (
          <button
            onClick={() => run("live_edit_editor_reset")}
            className="px-4 h-9 bg-zinc-900 border border-white/5 text-zinc-300 rounded-xl text-xs font-bold uppercase hover:bg-white/5 transition-all"
          >
            Use {editor.default_label}
          </button>
        )}
      </div>
      {error && <p className="text-[11.5px] text-red-400">{error}</p>}
    </div>
  );
}

export function LiveEditSection() {
  return (
    <SettingsSection icon={FileEdit} title="Live Edit">
      <LiveEditSettings />
    </SettingsSection>
  );
}

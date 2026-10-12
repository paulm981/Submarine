import { useEffect, useMemo, useRef, useState } from "react";
import { Settings, Palette, RefreshCw, List, Cloud, Minus, Plus, FileEdit, Server, Home } from "lucide-react";
import {
  MAX_FONT_SIZE, MIN_FONT_SIZE, clampFontSize, fontFamilyCss,
  isFontAvailable, primaryFontName, sanitizeFontFamily,
} from "../util/terminalFont";
import { invoke } from "@tauri-apps/api/core";
import type { LocalShells } from "./HomePane";
import CopyValue from "./CopyValue";
import FontPicker from "./FontPicker";
import { loadInstalledFonts } from "../util/systemFonts";
import { IS_ANDROID } from "../util/platform";

// Where this install keeps its data (see src-tauri/src/portable.rs). `portable`
// means a `submarine-data` folder next to the executable is in use; `warning`
// is set when such a folder exists but couldn't be written to.
interface StorageInfo {
  portable: boolean;
  data_dir: string;
  warning: string | null;
}

// The editor Live Edit opens remote files in (src-tauri/src/local_open.rs).
interface LiveEditEditor {
  chosen: string | null;
  default_label: string;
}

// How many sessions log in to the same jump host at once (multiexec.rs):
// opening a folder of servers behind one queues the rest. Kept by the
// backend, which does the queueing.
function JumpLoginLimit() {
  const [limit, setLimit] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<number>("get_jump_login_limit").then((n) => { setLimit(n); setDraft(String(n)); }).catch(() => setLimit(null));
  }, []);
  const save = () => {
    const n = Number(draft.trim());
    if (!Number.isInteger(n) || n < 0 || n > 256) {
      setError("A whole number from 0 to 256.");
      return;
    }
    setError(null);
    invoke<number>("set_jump_login_limit", { limit: n })
      .then((v) => { setLimit(v); setDraft(String(v)); })
      .catch((e) => setError(String(e)));
  };
  return (
    <div className="flex items-center justify-between gap-4">
      <div className="min-w-0">
        <div className="text-sm font-semibold text-zinc-100">Logins at a time per jump host</div>
        <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
          When many servers behind one jump host open at once (a folder opened as a wall), this many log in to it
          together and the rest wait their turn — a jump host turns away logins past its own limit (OpenSSH starts
          at 10). 0 means no limit. Servers reached directly aren't held back.
        </p>
        {error && <p className="text-[11px] text-red-400 mt-1">{error}</p>}
      </div>
      <input
        type="number"
        min={0}
        max={256}
        aria-label="Logins at a time per jump host"
        value={draft}
        disabled={limit === null}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => { if (e.key === "Enter") save(); }}
        className="shrink-0 w-20 h-9 px-2 bg-zinc-900/60 border border-white/10 rounded-lg text-[13px] text-zinc-50 text-center outline-none focus:border-primary/50"
      />
    </div>
  );
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
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
      <p className="text-sm text-zinc-400 leading-relaxed">
        Double-clicking a remote file opens a copy in this editor and uploads it each time you save.
        It never runs the file.
      </p>
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
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
      <p className="text-sm text-zinc-400 leading-relaxed">
        Home opens a terminal on this computer, in your home folder. New ones start this shell; the arrow
        beside + in Home starts any of the others.
      </p>
      <div className="space-y-1.5">
        <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Shell</span>
        <div className="flex flex-col gap-1">
          {info.shells.map((s) => (
            <label key={s.id} className="flex items-center gap-2.5 px-3 h-9 rounded-xl border border-white/5 bg-zinc-900/60 hover:bg-white/[0.04] cursor-pointer">
              <input
                type="radio"
                name="home-shell"
                checked={info.default_id === s.id}
                onChange={() => run("local_shell_set_default", { id: s.id })}
                className="accent-[var(--primary-hex)]"
              />
              <span className="text-[12.5px] text-zinc-200">{s.label}</span>
            </label>
          ))}
        </div>
      </div>
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

// Slider covers the everyday range; the number field takes anything 1–99.
const SLIDER_MAX = 40;

// Terminal font face + size. Both are drafts while you type: the size field
// can sit empty mid-edit (it used to snap back to 14 the moment it was
// cleared, which made small sizes impossible to type on Android), and the
// face is applied ~0.5 s after you stop typing instead of re-measuring every
// open terminal on each keystroke.
function TerminalFontSettings({ settings, setSettings }: any) {
  const size = clampFontSize(settings.terminalFontSize);
  const family: string = settings.terminalFontFamily || "";

  const [sizeDraft, setSizeDraft] = useState(String(size));
  useEffect(() => setSizeDraft(String(size)), [size]);
  const setSize = (n: number) =>
    setSettings((s: any) => ({ ...s, terminalFontSize: clampFontSize(n) }));

  const [familyDraft, setFamilyDraft] = useState(family);
  useEffect(() => setFamilyDraft(family), [family]);
  const commitTimer = useRef<number | null>(null);
  useEffect(() => () => { if (commitTimer.current) window.clearTimeout(commitTimer.current); }, []);
  const commitFamily = (v: string) => {
    if (commitTimer.current) { window.clearTimeout(commitTimer.current); commitTimer.current = null; }
    const clean = sanitizeFontFamily(v);
    setSettings((s: any) => (s.terminalFontFamily === clean ? s : { ...s, terminalFontFamily: clean }));
  };
  const onFamilyChange = (v: string) => {
    setFamilyDraft(v);
    if (commitTimer.current) window.clearTimeout(commitTimer.current);
    commitTimer.current = window.setTimeout(() => commitFamily(v), 500);
  };

  // Families installed on this device: the list the picker shows. The canvas
  // probe in isFontAvailable can miss a font the webview does draw (seen on
  // macOS, #59), and a font picked under "Installed on this device" must not
  // then be called missing. No warning until the list is in, so it can't
  // flash on and off; if the list can't be read, the probe decides alone.
  const [listed, setListed] = useState<Set<string> | null>(null);
  useEffect(() => {
    let alive = true;
    loadInstalledFonts().then(
      (fonts) => { if (alive) setListed(new Set(fonts.map((f) => f.family.toLowerCase()))); },
      () => { if (alive) setListed(new Set()); },
    );
    return () => { alive = false; };
  }, []);

  const installed = useMemo(
    () => listed === null || listed.has(primaryFontName(familyDraft).toLowerCase()) || isFontAvailable(familyDraft),
    [familyDraft, listed],
  );
  const previewFamily = fontFamilyCss(familyDraft);

  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-6 shadow-xl">
      {/* Font face */}
      <div className="space-y-2">
        <div className="flex justify-between items-center gap-2">
          <label htmlFor="term-font-family" className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Font</label>
          {familyDraft && (
            <button
              type="button"
              onClick={() => { setFamilyDraft(""); commitFamily(""); }}
              className="text-[11px] font-semibold text-zinc-400 hover:text-white transition-colors"
            >
              Use default
            </button>
          )}
        </div>
        <FontPicker
          value={familyDraft}
          onChange={onFamilyChange}
          onCommit={commitFamily}
          placeholder="Default monospace — pick from the list or type a font name"
        />
        {!installed && (
          <p className="text-[11.5px] text-amber-400/90">
            "{primaryFontName(familyDraft)}" doesn't seem to be installed on this device, so the default font is used instead.
          </p>
        )}
        <p className="text-[11.5px] text-zinc-500 leading-relaxed">
          Built-in fonts ship with the app and work on every device. Any font installed on this device works too, including Nerd Fonts for starship / powerline prompts.
        </p>
      </div>

      {/* Font size */}
      <div className="space-y-3 pt-5 border-t border-white/5">
        <div className="flex justify-between items-center gap-3">
          <label htmlFor="term-font-size" className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Font Size (px)</label>
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              aria-label="Smaller font"
              onClick={() => setSize(size - 1)}
              disabled={size <= MIN_FONT_SIZE}
              className="w-8 h-8 flex items-center justify-center bg-black border border-white/10 rounded-lg text-zinc-300 hover:text-white hover:border-primary/40 disabled:opacity-30 transition-colors"
            >
              <Minus size={14} />
            </button>
            <input
              id="term-font-size"
              type="text"
              inputMode="numeric"
              pattern="[0-9]*"
              value={sizeDraft}
              onChange={(e) => {
                const v = e.target.value.replace(/[^0-9]/g, "").slice(0, 2);
                setSizeDraft(v);
                const n = parseInt(v, 10);
                if (n >= MIN_FONT_SIZE) setSize(n);
              }}
              onBlur={() => setSizeDraft(String(size))}
              className="w-14 h-8 bg-black border border-white/10 rounded-lg px-2 text-[12px] font-bold text-white focus:border-primary/50 outline-none text-center"
            />
            <button
              type="button"
              aria-label="Larger font"
              onClick={() => setSize(size + 1)}
              disabled={size >= MAX_FONT_SIZE}
              className="w-8 h-8 flex items-center justify-center bg-black border border-white/10 rounded-lg text-zinc-300 hover:text-white hover:border-primary/40 disabled:opacity-30 transition-colors"
            >
              <Plus size={14} />
            </button>
          </div>
        </div>
        <input
          type="range"
          aria-label="Font size"
          min={MIN_FONT_SIZE}
          max={SLIDER_MAX}
          value={Math.min(size, SLIDER_MAX)}
          onChange={(e) => setSize(parseInt(e.target.value, 10))}
          className="w-full accent-primary"
        />
        <p className="text-[11.5px] text-zinc-500">Any size from {MIN_FONT_SIZE} to {MAX_FONT_SIZE} — type it in or use the − / + buttons.</p>
      </div>

      {/* Live preview */}
      <div className="rounded-xl bg-[#09090b] border border-white/5 px-3 py-2.5 overflow-x-auto custom-scrollbar">
        <pre
          className="m-0 whitespace-pre text-[#e4e4e7] leading-snug"
          style={{ fontFamily: previewFamily, fontSize: Math.min(size, 28) }}
        >
          <span className="text-primary">user@server</span>:~$ ls -la{"\n"}0O 1lI | {"{}"} [] =&gt; ✓
        </pre>
      </div>
    </div>
  );
}

const SettingsPanel = ({ settings, setSettings, onOpenLogs }: any) => {
  const accentColors = [
    { name: 'Light Blue', value: '#60a5fa' },
    { name: 'Sky', value: '#38bdf8' },
    { name: 'Cyan', value: '#22d3ee' },
    { name: 'Indigo', value: '#818cf8' },
    { name: 'Emerald', value: '#10b981' },
    { name: 'Rose', value: '#f43f5e' },
  ];

  const bgColors = [
    { name: 'Onyx', value: '#050505' },
    { name: 'Charcoal', value: '#0a0a0c' },
    { name: 'Slate', value: '#0f172a' },
    { name: 'Nord', value: '#2e3440' },
    { name: 'Dark Gray', value: '#1a1a1a' },
    { name: 'Deep Space', value: '#0d0d10' },
  ];

  // Read-only storage summary from the backend: where data lives and whether
  // a `submarine-data` folder next to the exe (portable mode) is in use.
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  useEffect(() => {
    invoke<StorageInfo>("get_storage_info").then(setStorage).catch(() => setStorage(null));
  }, []);

  return (
    <div className="flex-1 p-4 sm:p-10 overflow-y-auto custom-scrollbar animate-in">
      <header className="mb-6 sm:mb-10">
        <h2 className="text-xl sm:text-2xl font-bold text-white tracking-tight flex items-center gap-3">
          <Settings size={24} className="text-primary" /> Settings
        </h2>
        <p className="text-[13px] text-zinc-400 mt-2">
          Tweak how Submarine looks and feels.
          {' '}
          <span className="text-zinc-500 italic">Per-device — preferences live in this machine's local storage and don't sync with your cloud profile.</span>
        </p>
      </header>

      {/* Columns rather than a grid: the sections are very different heights, and
          a 2-col grid rows them up so a short card leaves a tall empty gap beside
          it. Column flow packs them, and break-inside-avoid keeps a card whole. */}
      <div className="columns-1 md:columns-2 gap-4 sm:gap-8">
        {/* Appearance Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <Palette size={14} /> UI Customization
          </div>

          <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-8 shadow-xl">
            {/* Accent Color */}
            <div className="space-y-4">
              <div className="flex justify-between items-center">
                <label className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Primary Accent Color</label>
                <div className="flex items-center gap-2">
                  <input 
                    type="color" 
                    value={settings.primaryColor} 
                    onChange={(e) => setSettings({ ...settings, primaryColor: e.target.value })}
                    className="w-6 h-6 rounded-md bg-transparent cursor-pointer border-none p-0"
                  />
                  <input 
                    type="text" 
                    value={settings.primaryColor} 
                    onChange={(e) => setSettings({ ...settings, primaryColor: e.target.value })}
                    className="w-20 h-6 bg-black border border-white/10 rounded px-1.5 text-[10px] font-mono text-zinc-400 focus:border-primary/50 outline-none"
                  />
                </div>
              </div>
              <div className="grid grid-cols-6 gap-3">
                {accentColors.map(c => (
                  <button
                    key={c.value}
                    onClick={() => setSettings({ ...settings, primaryColor: c.value })}
                    className={`w-full aspect-square rounded-xl border-2 transition-all ${settings.primaryColor === c.value ? 'border-white scale-110 shadow-lg' : 'border-transparent hover:scale-105'}`}
                    style={{ backgroundColor: c.value }}
                    title={c.name}
                  />
                ))}
              </div>
            </div>

            {/* Background Theme */}
            <div className="space-y-4 pt-6 border-t border-white/5">
              <div className="flex justify-between items-center">
                <label className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Background Theme</label>
                <div className="flex items-center gap-2">
                  <input 
                    type="color" 
                    value={settings.backgroundColor} 
                    onChange={(e) => setSettings({ ...settings, backgroundColor: e.target.value })}
                    className="w-6 h-6 rounded-md bg-transparent cursor-pointer border-none p-0"
                  />
                  <input 
                    type="text" 
                    value={settings.backgroundColor} 
                    onChange={(e) => setSettings({ ...settings, backgroundColor: e.target.value })}
                    className="w-20 h-6 bg-black border border-white/10 rounded px-1.5 text-[10px] font-mono text-zinc-400 focus:border-primary/50 outline-none"
                  />
                </div>
              </div>
              <div className="grid grid-cols-6 gap-3">
                {bgColors.map(c => (
                  <button
                    key={c.value}
                    onClick={() => setSettings({ ...settings, backgroundColor: c.value })}
                    className={`w-full aspect-square rounded-xl border-2 transition-all ${settings.backgroundColor === c.value ? 'border-white scale-110 shadow-lg' : 'border-transparent hover:scale-105'}`}
                    style={{ backgroundColor: c.value }}
                    title={c.name}
                  />
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* Terminal Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <Settings size={14} /> Terminal Configuration
          </div>
          
          <TerminalFontSettings settings={settings} setSettings={setSettings} />
        </section>

        {/* Servers Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <Server size={14} /> Servers
          </div>

          <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-zinc-100">Show each server's OS</div>
                <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
                  The first time a server connects, Submarine reads <span className="font-mono text-zinc-300">/etc/os-release</span>{' '}
                  (or <span className="font-mono text-zinc-300">uname</span> / <span className="font-mono text-zinc-300">ver</span>)
                  in the background for the badge beside its name. Turn off where every command is recorded or a
                  key only allows one command.
                </p>
              </div>
              <button
                role="switch"
                aria-checked={settings.detectOs !== false}
                onClick={() => setSettings((s: any) => ({ ...s, detectOs: !(s.detectOs !== false) }))}
                className={`relative shrink-0 w-11 h-6 rounded-full transition-colors ${settings.detectOs !== false ? 'bg-primary' : 'bg-zinc-700'}`}
              >
                <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${settings.detectOs !== false ? 'left-[22px]' : 'left-0.5'}`} />
              </button>
            </div>
            <JumpLoginLimit />
          </div>
        </section>

        {/* Updates Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <RefreshCw size={14} /> Updates
          </div>

          <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
            <p className="text-sm text-zinc-400 leading-relaxed">
              The profile screen says when there's a new release of this build. Built from source, it also says when
              the fork's main has changes to pull and rebuild.
            </p>
            <div className="flex items-center justify-between gap-4 border-t border-white/5 pt-4">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-zinc-100">Watch the original project</div>
                <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
                  For whoever maintains the fork: also say when SinaXhpm/Submarine has changes the fork doesn't
                  have yet, so you can use Sync fork on GitHub.
                </p>
              </div>
              <button
                role="switch"
                aria-checked={!!settings.watchUpstream}
                onClick={() => setSettings((s: any) => ({ ...s, watchUpstream: !s.watchUpstream }))}
                className={`relative shrink-0 w-11 h-6 rounded-full transition-colors ${settings.watchUpstream ? 'bg-primary' : 'bg-zinc-700'}`}
              >
                <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${settings.watchUpstream ? 'left-[22px]' : 'left-0.5'}`} />
              </button>
            </div>
          </div>
        </section>

        {/* Home Terminal Section */}
        {!IS_ANDROID && (
          <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
            <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
              <Home size={14} /> Home Terminal
            </div>
            <HomeTerminalSettings />
          </section>
        )}

        {/* Live Edit Section */}
        {!IS_ANDROID && (
          <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
            <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
              <FileEdit size={14} /> Live Edit
            </div>
            <LiveEditSettings />
          </section>
        )}

        {/* Activity Section — moved out of the sidebar so the top-level
            navigation stays focused on primary workflows. Logs are diagnostic
            only; keeping them one click deep here cleans up the sidebar on
            mobile (six icons → five) without burying the data. */}
        {onOpenLogs && (
          <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
            <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
              <List size={14} /> Activity Log
            </div>

            <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
              <p className="text-sm text-zinc-400 leading-relaxed">
                Diagnostic timeline of what the app has been doing this session. Useful for confirming a connection actually failed or watching a transfer's progress in retrospect.
              </p>
              <button
                onClick={onOpenLogs}
                className="px-4 h-9 bg-primary/10 border border-primary/30 text-primary rounded-xl text-xs font-bold uppercase hover:bg-primary hover:text-zinc-950 transition-all w-full flex items-center justify-center gap-2"
              >
                <List size={14} /> View Activity Log
              </button>
            </div>
          </section>
        )}

        {/* Cloud Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <Cloud size={14} /> Cloud Sync
          </div>

          <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="text-sm font-semibold text-zinc-100">Auto-sync</div>
                <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
                  Keep open profiles synced in the background — shortly after you make a change, when
                  you return to the window, and every few minutes to pull in others' edits on shared
                  profiles. Turn off to sync only with the manual button.
                </p>
              </div>
              <button
                role="switch"
                aria-checked={settings.autoSync !== false}
                onClick={() => setSettings((s: any) => ({ ...s, autoSync: !(s.autoSync !== false) }))}
                className={`relative shrink-0 w-11 h-6 rounded-full transition-colors ${settings.autoSync !== false ? 'bg-primary' : 'bg-zinc-700'}`}
              >
                <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all ${settings.autoSync !== false ? 'left-[22px]' : 'left-0.5'}`} />
              </button>
            </div>

            {settings.autoSync !== false && (
              <div className="border-t border-white/5 pt-4 text-[12px] text-zinc-400">
                Currently checking every{' '}
                <span className="text-zinc-100 font-semibold">{settings.syncIntervalMin ?? 5} min</span>. Change it
                on the Profile tab, next to Sync now.
              </div>
            )}
          </div>
        </section>

        {/* Maintenance Section */}
        <section className="break-inside-avoid space-y-3 mb-4 sm:mb-8">
          <div className="flex items-center gap-2 text-zinc-400 font-bold uppercase tracking-widest text-xs">
            <RefreshCw size={14} /> Maintenance
          </div>

          <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-4 shadow-xl">
            {storage && (
              <div className="space-y-1.5 pb-4 border-b border-white/5">
                <div className="flex items-center gap-2">
                  <span className="text-[11px] font-black text-zinc-500 uppercase tracking-wider">Data folder</span>
                  {storage.portable && (
                    <span className="px-1.5 py-0.5 rounded-md text-[10px] font-bold uppercase tracking-wider text-primary bg-primary/10 border border-primary/30">
                      Portable
                    </span>
                  )}
                </div>
                <CopyValue value={storage.data_dir} className="max-w-full text-zinc-300">
                  <span className="font-mono text-[11.5px] break-all select-text">{storage.data_dir}</span>
                </CopyValue>
                {storage.warning && (
                  <p className="text-[11.5px] text-amber-400/90 leading-relaxed">{storage.warning}</p>
                )}
              </div>
            )}
            <p className="text-sm text-zinc-400 leading-relaxed">
              These preferences are persisted in your local environment. Resetting will revert all UI aesthetics to factory defaults.
            </p>
            <button
              onClick={() => {
                if(window.confirm('Reset all UI customizations?')) {
                  setSettings((s: any) => ({ ...s, primaryColor: '#60a5fa', backgroundColor: '#0a0a0c', terminalFontSize: 14, terminalFontFamily: '' }));
                }
              }}
              className="px-4 h-9 bg-zinc-900 border border-white/5 text-zinc-300 rounded-xl text-xs font-bold uppercase hover:bg-white/5 transition-all w-full"
            >
              Reset to Defaults
            </button>
          </div>
        </section>
      </div>
    </div>
  );
};

export default SettingsPanel;

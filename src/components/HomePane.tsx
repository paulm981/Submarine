import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, Plus, TerminalSquare, X } from "lucide-react";
import TerminalView from "./TerminalView";

// The Home view: terminals on this computer (src-tauri/src/local_term.rs),
// started in the home folder — PowerShell, Command Prompt, WSL or Git Bash on
// Windows, the login shell elsewhere; which one is set under Settings › Home
// terminal. Not server sessions: they never join the session tabs, a
// broadcast or the Wall, and they stay open while the user works elsewhere
// (DesktopApp keeps this pane mounted once it has been shown).

export interface LocalShell { id: string; label: string; }
export interface LocalShells { shells: LocalShell[]; default_id: string; custom: string | null; }

interface HomeTab {
  id: string;
  /// local_shells id; null = the default at the time it opened.
  shell: string | null;
  label: string;
  ended: boolean;
}

const newId = () => `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

export const HomePane = ({ visible }: { visible: boolean }) => {
  const [tabs, setTabs] = useState<HomeTab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [shells, setShells] = useState<LocalShells | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [focusTick, setFocusTick] = useState(0);
  const opened = useRef(false);

  const loadShells = () =>
    invoke<LocalShells>("local_shells").then(setShells).catch(() => setShells(null));

  const openShell = (shell: string | null) => {
    const label = shell ? (shells?.shells.find((s) => s.id === shell)?.label ?? "Terminal") : "Terminal";
    const id = newId();
    setTabs((prev) => [...prev, { id, shell, label, ended: false }]);
    setActive(id);
    setMenu(null);
  };

  // The first time Home is shown, a terminal in the default shell — the
  // "home pane" — so there is a prompt to type at straight away.
  useEffect(() => {
    if (!visible || opened.current) return;
    opened.current = true;
    loadShells();
    openShell(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible]);

  // Coming back to Home puts the caret in its terminal.
  useEffect(() => { if (visible) setFocusTick((n) => n + 1); }, [visible]);

  // A shell that exits leaves its tab, marked, with its last output.
  const tabIds = tabs.map((t) => t.id).join(",");
  useEffect(() => {
    const unlisten = tabIds.split(",").filter(Boolean).map((id) =>
      listen(`terminal-closed-${id}`, () =>
        setTabs((prev) => prev.map((x) => (x.id === id ? { ...x, ended: true } : x))),
      ),
    );
    return () => { unlisten.forEach((p) => p.then((u) => u())); };
  }, [tabIds]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => { window.removeEventListener("click", close); window.removeEventListener("blur", close); };
  }, [menu]);

  // Closing a tab ends its shell (TerminalView closes it on unmount); the
  // tab to its left takes over.
  const closeTab = (id: string) => {
    const at = tabs.findIndex((t) => t.id === id);
    const next = tabs.filter((t) => t.id !== id);
    setTabs(next);
    if (active === id) setActive(next.length ? next[Math.max(0, at - 1)].id : null);
  };

  const defaultLabel = shells?.shells.find((s) => s.id === shells.default_id)?.label;

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="shrink-0 h-12 px-3 flex items-center gap-1 border-b border-white/5 bg-background">
        <div className="flex items-center gap-1 overflow-x-auto no-scrollbar flex-1 min-w-0" onWheel={(e) => { e.currentTarget.scrollLeft += e.deltaY; }}>
          {tabs.map((t) => (
            <div key={t.id} className="group relative flex items-center shrink-0">
              <button
                onClick={() => { setActive(t.id); setFocusTick((n) => n + 1); }}
                title={t.ended ? `${t.label} — ended` : t.label}
                className={`h-8 pl-3 pr-8 rounded-lg flex items-center gap-2 text-[11px] font-bold tracking-wide transition-all ${
                  active === t.id
                    ? "bg-primary/10 text-primary border border-primary/20 shadow-inner"
                    : "text-zinc-300 bg-white/[0.04] border border-white/10 hover:bg-white/[0.08] hover:border-white/20 hover:text-white"
                } ${t.ended ? "opacity-60" : ""}`}
              >
                <TerminalSquare size={14} />
                {t.label}
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}
                aria-label="Close terminal"
                className="absolute right-1 top-1/2 -translate-y-1/2 w-5 h-5 flex items-center justify-center rounded opacity-0 group-hover:opacity-100 hover:bg-white/5 hover:text-red-400 text-zinc-500 transition-all"
              >
                <X size={12} />
              </button>
            </div>
          ))}
          {/* "+" opens the default shell; the arrow beside it, any of them. */}
          <div className="flex items-center ml-1 shrink-0 rounded-lg border border-dashed border-white/10">
            <button
              onClick={() => openShell(null)}
              title={defaultLabel ? `New ${defaultLabel} terminal` : "New terminal"}
              className="h-8 w-8 flex items-center justify-center text-zinc-500 hover:bg-white/10 hover:text-white rounded-l-lg transition-all"
            >
              <Plus size={14} />
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                const rect = e.currentTarget.getBoundingClientRect();
                loadShells();
                setMenu((m) => (m ? null : { x: rect.left, y: rect.bottom + 4 }));
              }}
              title="Choose a shell"
              className="h-8 w-5 flex items-center justify-center text-zinc-500 hover:bg-white/10 hover:text-white rounded-r-lg border-l border-dashed border-white/10 transition-all"
            >
              <ChevronDown size={12} />
            </button>
          </div>
        </div>
      </div>

      <div className="flex-1 relative min-h-0 bg-black/40">
        {tabs.length === 0 && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-center px-6">
            <TerminalSquare size={28} className="text-zinc-600" />
            <p className="text-[12px] text-zinc-500 max-w-sm leading-relaxed">
              A terminal on this computer, in your home folder. Pick the shell under Settings › Home terminal.
            </p>
            <button
              onClick={() => openShell(null)}
              className="px-4 h-8 rounded-lg text-[11px] font-bold uppercase tracking-wider bg-primary/15 border border-primary/40 text-primary hover:bg-primary/25"
            >
              Open {defaultLabel ?? "a terminal"}
            </button>
          </div>
        )}
        {tabs.map((t) => (
          <div key={t.id} className={`absolute inset-0 ${active === t.id ? "opacity-100 z-10" : "opacity-0 z-0 pointer-events-none"}`}>
            <TerminalView
              sessionId="home"
              terminalId={t.id}
              isActive={visible && active === t.id}
              isActiveView={visible}
              focusSignal={focusTick}
              local={{
                shell: t.shell,
                onOpened: (label) => setTabs((prev) => prev.map((x) => (x.id === t.id ? { ...x, label } : x))),
              }}
            />
          </div>
        ))}
      </div>

      {menu && createPortal(
        <div
          className="fixed z-[9999] min-w-[220px] py-1 bg-[#18181c] border border-white/10 rounded-lg shadow-2xl"
          style={{ left: Math.min(menu.x, window.innerWidth - 240), top: menu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          {(shells?.shells ?? []).map((s) => (
            <button
              key={s.id}
              onClick={() => openShell(s.id)}
              className="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[12px] text-zinc-200 hover:bg-white/10"
            >
              <TerminalSquare size={12} className="text-zinc-500" />
              <span className="flex-1">{s.label}</span>
              {s.id === shells?.default_id && <span className="text-[9.5px] font-bold uppercase tracking-wider text-primary/70">Default</span>}
            </button>
          ))}
          {!shells && <div className="px-3 py-1.5 text-[12px] text-zinc-500">Looking for shells…</div>}
        </div>,
        document.body,
      )}
    </div>
  );
};

export default HomePane;

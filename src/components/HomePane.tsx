import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { Bot, ChevronDown, Plus, TerminalSquare, X } from "lucide-react";
import TerminalView from "./TerminalView";

// The Home view: terminals on this computer (src-tauri/src/local_term.rs),
// started in the home folder — PowerShell, Command Prompt, WSL or Git Bash on
// Windows, the login shell elsewhere; which one is set under Settings › Home
// terminal. Not server sessions: they never join the session tabs, a
// broadcast or the Wall, and they stay open while the user works elsewhere
// (DesktopApp keeps this pane mounted once it has been shown).
//
// The menu's AI tools heading (#32) lists the AI tools Home can start —
// Claude Code — while AI tools are on in Settings and it's installed. Picked,
// it opens in a tab of its own, already connected to Submarine; closing the
// tab ends its access.

// A shell Home can start (local_term.rs): `group` is the heading it's listed
// under — "Windows" and "WSL" on Windows, "This computer" elsewhere, "Your
// choice" for a program the user picked — and `detail` what tells it apart
// ("default" for WSL's default distribution, "your login shell", a path).
export interface LocalShell { id: string; label: string; group: string; detail: string | null; }
// `notes`: what to say under a heading with nothing to list (WSL not
// installed, or no Linux in it yet).
export interface ShellNote { group: string; text: string; }
export interface LocalShells { shells: LocalShell[]; notes: ShellNote[]; default_id: string; custom: string | null; custom_missing: boolean; ai_tools: LocalShell[]; }

export interface ShellGroup { name: string; shells: LocalShell[]; notes: string[]; }

/// The shells under their headings, in the order they're listed, each with
/// its notes. The Home menu and Settings show the same list this way.
export const shellGroups = (info: LocalShells): ShellGroup[] => {
  const groups: ShellGroup[] = [];
  const get = (name: string) => {
    let g = groups.find((x) => x.name === name);
    if (!g) groups.push((g = { name, shells: [], notes: [] }));
    return g;
  };
  for (const s of info.shells) get(s.group).shells.push(s);
  for (const n of info.notes ?? []) get(n.group).notes.push(n.text);
  return groups;
};

interface HomeTab {
  id: string;
  /// list_local_shells id; null = the default at the time it opened.
  shell: string | null;
  label: string;
  ended: boolean;
}

const newId = () => `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

// `onLiveShells`: how many of its shells are still running, for the close and
// lock confirmations (0 once the pane unmounts).
export const HomePane = ({ visible, onLiveShells }: { visible: boolean; onLiveShells?: (n: number) => void }) => {
  const [tabs, setTabs] = useState<HomeTab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [shells, setShells] = useState<LocalShells | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [focusTick, setFocusTick] = useState(0);
  const opened = useRef(false);

  const loadShells = () =>
    invoke<LocalShells>("list_local_shells").then(setShells).catch(() => setShells(null));

  const openShell = (shell: string | null) => {
    const known = shell ? [...(shells?.shells ?? []), ...(shells?.ai_tools ?? [])].find((s) => s.id === shell) : undefined;
    const label = known?.label ?? "Terminal";
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

  const live = tabs.filter((t) => !t.ended).length;
  const onLiveShellsRef = useRef(onLiveShells);
  onLiveShellsRef.current = onLiveShells;
  useEffect(() => { onLiveShellsRef.current?.(live); }, [live]);
  useEffect(() => () => onLiveShellsRef.current?.(0), []);

  // A shell that exits leaves its tab, marked, with its last output. One
  // listener per tab for as long as the tab is open, so an exit can't slip
  // through while the listeners are being redone.
  const closedListeners = useRef(new Map<string, Promise<UnlistenFn>>());
  useEffect(() => {
    const ids = new Set(tabs.map((t) => t.id));
    for (const id of ids) {
      if (closedListeners.current.has(id)) continue;
      closedListeners.current.set(id, listen(`terminal-closed-${id}`, () =>
        setTabs((prev) => prev.map((x) => (x.id === id ? { ...x, ended: true } : x))),
      ));
    }
    for (const [id, unlisten] of closedListeners.current) {
      if (ids.has(id)) continue;
      unlisten.then((u) => u());
      closedListeners.current.delete(id);
    }
  }, [tabs]);
  useEffect(() => () => {
    for (const unlisten of closedListeners.current.values()) unlisten.then((u) => u());
    closedListeners.current.clear();
  }, []);

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
                {t.shell?.startsWith("ai:") ? <Bot size={14} /> : <TerminalSquare size={14} />}
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
                const rect = e.currentTarget.getBoundingClientRect();
                loadShells();
                setMenu({ x: rect.left, y: rect.bottom + 4 });
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
              disabled={t.ended}
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
        <>
          <div
            className="fixed inset-0 z-[9998]"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}
          />
          <div
            style={{ left: Math.min(menu.x, window.innerWidth - 280), top: menu.y }}
            className="fixed z-[9999] w-[260px] max-h-[70vh] overflow-y-auto custom-scrollbar bg-[#15151a] border border-white/10 rounded-lg shadow-2xl py-1 text-[11.5px]"
          >
            {shells && (() => {
              const groups = shellGroups(shells);
              // Headings only once there's more than one kind to tell apart.
              const headed = groups.length > 1 || groups.some((g) => g.notes.length > 0);
              return groups.map((g, i) => (
                <div key={g.name} className={i > 0 ? "mt-1 pt-1 border-t border-white/5" : ""}>
                  {headed && (
                    <div className="px-3 pt-1 pb-0.5 text-[9.5px] font-bold uppercase tracking-wider text-zinc-500">{g.name}</div>
                  )}
                  {g.shells.map((s) => (
                    <button
                      key={s.id}
                      onClick={() => openShell(s.id)}
                      title={s.detail ? `${s.label} — ${s.detail}` : s.label}
                      className="w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left"
                    >
                      <TerminalSquare size={13} className="text-zinc-500 shrink-0" />
                      <span className="flex-1 min-w-0">
                        <span className="block truncate">{s.label}</span>
                        {s.detail && <span className="block truncate text-[10px] text-zinc-500">{s.detail}</span>}
                      </span>
                      {s.id === shells.default_id && <span className="shrink-0 text-[9.5px] font-bold uppercase tracking-wider text-primary/70">Default</span>}
                    </button>
                  ))}
                  {g.notes.map((n, j) => (
                    <div key={j} className="px-3 py-1 text-[10.5px] text-zinc-500 leading-snug">{n}</div>
                  ))}
                </div>
              ));
            })()}
            {shells && shells.ai_tools?.length > 0 && (
              <div className="mt-1 pt-1 border-t border-white/5">
                <div className="px-3 pt-1 pb-0.5 text-[9.5px] font-bold uppercase tracking-wider text-zinc-500">AI tools</div>
                {shells.ai_tools.map((s) => (
                  <button
                    key={s.id}
                    onClick={() => openShell(s.id)}
                    title={`${s.label}, connected to Submarine — ${s.detail ?? ""}`}
                    className="w-full flex items-center gap-2.5 px-3 py-1.5 hover:bg-white/[0.06] text-zinc-200 hover:text-white text-left"
                  >
                    <Bot size={13} className="text-zinc-500 shrink-0" />
                    <span className="flex-1 min-w-0">
                      <span className="block truncate">{s.label}</span>
                      {s.detail && <span className="block truncate text-[10px] text-zinc-500">{s.detail}</span>}
                    </span>
                  </button>
                ))}
              </div>
            )}
            {!shells && <div className="px-3 py-1.5 text-zinc-500">Looking for shells…</div>}
          </div>
        </>,
        document.body,
      )}
    </div>
  );
};

export default HomePane;

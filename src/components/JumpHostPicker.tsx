import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronDown, Search } from "lucide-react";
import { folderOptions, FolderRow } from "../util/folderPath";

// The server form's "Jump via" (#102): a search box over the profile's SSH
// servers, since a plain drop-down of hundreds of them can only be scrolled.
// Servers already used as a jump host by another server come first.

type Server = { id: number; name: string; host: string; folder_id?: number | null; jump_host_id?: number | null; protocol?: string | null };
type Option = { id: number; name: string; host: string; folder: string; used: boolean };

// More rows than anyone scrolls through; typing narrows it.
const MAX_ROWS = 200;

const matches = (o: Option, words: string[]) => {
  const text = `${o.name} ${o.host} ${o.folder}`.toLowerCase();
  return words.every((w) => text.includes(w));
};

export const JumpHostPicker = ({ value, onChange, servers, folders, selfId, directLabel, label }: {
  value: string;
  onChange: (id: string) => void;
  servers: Server[];
  folders: FolderRow[];
  selfId?: number;
  directLabel: string;
  label: string;
}) => {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const boxRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const listId = useId();

  const options = useMemo(() => {
    const folderLabel = new Map(folderOptions(folders || []).map((f) => [f.id, f.label]));
    const usedIds = new Set((servers || []).map((s) => s.jump_host_id).filter((id): id is number => id != null));
    return (servers || [])
      .filter((s) => s.id !== selfId && (s.protocol ?? "ssh") === "ssh")
      .map((s): Option => ({
        id: s.id, name: s.name, host: s.host,
        folder: s.folder_id != null ? folderLabel.get(s.folder_id) ?? "" : "",
        used: usedIds.has(s.id),
      }))
      .sort((a, b) => Number(b.used) - Number(a.used) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }));
  }, [servers, folders, selfId]);

  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return (words.length ? options.filter((o) => matches(o, words)) : options).slice(0, MAX_ROWS);
  }, [options, query]);

  // Row 0 is "Direct", then the servers.
  const rowCount = shown.length + 1;
  const selected = options.find((o) => String(o.id) === value);
  const closedText = selected ? `${selected.name} (${selected.host})` : value ? "A server that's no longer here" : directLabel;

  const pick = (id: string) => {
    onChange(id);
    setOpen(false);
    setQuery("");
  };

  const openList = () => {
    setOpen(true);
    setQuery("");
    const at = shown.findIndex((o) => String(o.id) === value);
    setActive(at >= 0 ? at + 1 : 0);
  };

  // A click anywhere else closes the list.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) { setOpen(false); setQuery(""); }
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open]);

  // Keep the active row in view.
  useEffect(() => {
    if (!open) return;
    listRef.current?.querySelector(`[data-row="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter") { e.preventDefault(); openList(); }
      return;
    }
    if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(a + 1, rowCount - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(a - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); pick(active === 0 ? "" : String(shown[active - 1].id)); }
    // Esc closes the list only, not the panel around it.
    else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setOpen(false); setQuery(""); }
    else if (e.key === "Tab") { setOpen(false); setQuery(""); }
  };

  const firstOther = shown.findIndex((o) => !o.used);
  const anyUsed = shown.some((o) => o.used);

  return (
    <div ref={boxRef} className="relative flex-1 min-w-0">
      <div className="flex items-center h-9 bg-[#1a1a1e] rounded-lg px-3 border border-white/10 focus-within:border-primary/50 focus-within:bg-[#232328] transition-all shadow-inner">
        {open && <Search size={13} className="text-zinc-500 shrink-0 mr-2" />}
        <input
          ref={inputRef}
          role="combobox"
          aria-label={label}
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={open ? `${listId}-${active}` : undefined}
          aria-autocomplete="list"
          className={`flex-1 min-w-0 bg-transparent outline-none text-[12px] ${open || value ? "text-zinc-300" : "text-zinc-500"} placeholder:text-zinc-500`}
          value={open ? query : closedText}
          placeholder={open ? "Search by name, address or folder" : undefined}
          onFocus={() => { if (!open) openList(); }}
          onClick={() => { if (!open) openList(); }}
          onChange={(e) => { setQuery(e.target.value); setActive(e.target.value ? 1 : 0); if (!open) setOpen(true); }}
          onKeyDown={onKey}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          autoCorrect="off"
        />
        <ChevronDown size={14} className="text-zinc-500 shrink-0 ml-2" aria-hidden />
      </div>
      {open && (
        <ul
          ref={listRef}
          id={listId}
          role="listbox"
          aria-label={label}
          className="absolute z-50 left-0 right-0 mt-1 max-h-72 overflow-y-auto rounded-lg border border-white/10 bg-[#1a1a1e] shadow-2xl py-1"
        >
          <Row id={`${listId}-0`} row={0} active={active === 0} selected={!value} onPick={() => pick("")} onHover={() => setActive(0)}>
            <span className="text-zinc-400">{directLabel}</span>
          </Row>
          {shown.map((o, i) => (
            <li key={o.id} role="presentation">
              {i === 0 && anyUsed && <GroupHeading>Used as jump hosts</GroupHeading>}
              {i === firstOther && anyUsed && <GroupHeading>Other servers</GroupHeading>}
              <ul role="presentation">
                <Row id={`${listId}-${i + 1}`} row={i + 1} active={active === i + 1} selected={String(o.id) === value}
                     onPick={() => pick(String(o.id))} onHover={() => setActive(i + 1)}>
                  <span className="text-zinc-200 truncate">{o.name}</span>
                  <span className="text-zinc-500 truncate"> · {o.host}{o.folder ? ` · ${o.folder}` : ""}</span>
                </Row>
              </ul>
            </li>
          ))}
          {shown.length === 0 && (
            <li role="presentation" className="px-3 py-2 text-[11px] text-zinc-500">No SSH server matches "{query}".</li>
          )}
          {shown.length === MAX_ROWS && (
            <li role="presentation" className="px-3 py-1.5 text-[10.5px] text-zinc-500">Showing the first {MAX_ROWS}. Type to narrow the list.</li>
          )}
        </ul>
      )}
    </div>
  );
};

const GroupHeading = ({ children }: { children: React.ReactNode }) => (
  <div className="px-3 pt-2 pb-0.5 text-[9.5px] font-bold uppercase tracking-wider text-zinc-500">{children}</div>
);

const Row = ({ id, row, active, selected, onPick, onHover, children }: {
  id: string; row: number; active: boolean; selected: boolean;
  onPick: () => void; onHover: () => void; children: React.ReactNode;
}) => (
  <li
    id={id}
    data-row={row}
    role="option"
    aria-selected={selected}
    // Picking on mousedown keeps the focus in the search box.
    onMouseDown={(e) => { e.preventDefault(); onPick(); }}
    onMouseMove={onHover}
    className={`px-3 py-1.5 text-[12px] cursor-pointer flex min-w-0 ${active ? "bg-white/10" : ""} ${selected ? "font-semibold" : ""}`}
  >
    {children}
  </li>
);

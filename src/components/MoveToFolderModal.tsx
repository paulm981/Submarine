import { useEffect, useMemo, useState } from "react";
import { FolderInput, Folder, Search, X } from "lucide-react";
import { FolderRow, folderOptions } from "../util/folderPath";

// "Move to…" for the session tree: every folder as its path, searchable, plus
// the top level. With a few hundred imported sessions in dozens of folders, a
// list to type into beats a menu to scroll. `exclude` holds the folders that
// can't take what's moving (a folder being moved, and everything inside it).
const MoveToFolderModal = ({ title, folders, exclude, onPick, onClose }: {
  title: string;
  folders: FolderRow[];
  exclude: Set<number>;
  onPick: (folderId: number | null) => void;
  onClose: () => void;
}) => {
  const [q, setQ] = useState("");
  const options = useMemo(() => folderOptions(folders).filter((o) => !exclude.has(o.id)), [folders, exclude]);
  const shown = q.trim() ? options.filter((o) => o.label.toLowerCase().includes(q.trim().toLowerCase())) : options;
  const item = "w-full flex items-center gap-2.5 px-3 py-1.5 rounded-md text-left text-[12px] text-zinc-200 hover:bg-white/[0.06] hover:text-white";
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={onClose}>
      <div role="dialog" aria-modal="true" aria-label={title} className="w-full max-w-md max-h-[80vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <FolderInput size={13} className="text-indigo-300" /> {title}
          </span>
          <button onClick={onClose} className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>
        <div className="shrink-0 p-3 border-b border-white/5">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" size={13} />
            <input
              autoFocus
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.nativeEvent.isComposing && shown.length === 1) onPick(shown[0].id);
              }}
              placeholder="Find a folder…"
              className="w-full h-9 pl-8 pr-3 bg-zinc-900/60 border border-white/10 rounded-lg text-[12.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600"
            />
          </div>
        </div>
        <div className="flex-1 overflow-y-auto custom-scrollbar p-2">
          {!q.trim() && (
            <button onClick={() => onPick(null)} className={item}>
              <Folder size={13} className="text-zinc-500" /> <span className="italic text-zinc-400">Top level</span>
            </button>
          )}
          {shown.map((o) => (
            <button key={o.id} onClick={() => onPick(o.id)} className={item} title={o.label}>
              <Folder size={13} className="shrink-0 text-indigo-300" /> <span className="truncate">{o.label}</span>
            </button>
          ))}
          {shown.length === 0 && q.trim() && <div className="px-3 py-4 text-center text-[11.5px] text-zinc-600 italic">No folder matches.</div>}
        </div>
      </div>
    </div>
  );
};

export default MoveToFolderModal;

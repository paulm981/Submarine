import { useState } from "react";
import { AlertTriangle, MonitorSmartphone, X } from "lucide-react";
import { folderOptions } from "../util/folderPath";

// Add or edit a Remote Desktop link (src-tauri/src/rdp.rs). The link only
// stores where to connect; Windows' Remote Desktop does the rest and keeps the
// password itself when "Remember me" is ticked.
export type RdpLink = {
  id?: number | null;
  name: string;
  host: string;
  port: number;
  username: string;
  folder_id: number | null;
  notes: string;
};

const field = "w-full h-9 px-3 bg-zinc-900/60 border border-white/10 rounded-lg text-[12.5px] text-zinc-50 outline-none focus:border-primary/50";
const label = "text-[10px] font-bold text-zinc-300 uppercase tracking-wider ml-0.5";

const RdpLinkModal = ({ link, folders, onSave, onClose }: {
  link: RdpLink;
  folders: any[];
  onSave: (link: RdpLink) => Promise<void>;
  onClose: () => void;
}) => {
  const [draft, setDraft] = useState<RdpLink>(link);
  const [portText, setPortText] = useState(String(link.port || 3389));
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<RdpLink>) => setDraft((d) => ({ ...d, ...patch }));

  const save = async () => {
    setErr(null);
    const port = parseInt(portText, 10);
    if (!draft.host.trim()) { setErr("Host is required"); return; }
    if (!port || port < 1 || port > 65535) { setErr("Port must be 1–65535"); return; }
    setBusy(true);
    try {
      await onSave({ ...draft, name: draft.name.trim() || draft.host.trim(), host: draft.host.trim(), port });
      onClose();
    } catch (e: any) {
      setErr(typeof e === "string" ? e : (e?.message || String(e)));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={onClose}>
      <div className="w-full max-w-md max-h-[90vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <MonitorSmartphone size={13} className="text-sky-300" /> {link.id ? "Edit Remote Desktop link" : "New Remote Desktop link"}
          </span>
          <button onClick={onClose} className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>

        <div className="flex-1 overflow-y-auto custom-scrollbar p-5 space-y-3">
          {err && (
            <div className="px-3 py-2 bg-rose-500/15 border border-rose-500/30 rounded text-rose-200 text-[11.5px] font-mono flex items-center gap-2">
              <AlertTriangle size={12} /> {err}
            </div>
          )}

          <div className="space-y-1">
            <label className={label}>Name</label>
            <input value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder="e.g. Build box (defaults to the host)" className={field} autoFocus />
          </div>

          <div className="grid grid-cols-4 gap-2">
            <div className="col-span-3 space-y-1">
              <label className={label}>Host</label>
              <input value={draft.host} onChange={(e) => set({ host: e.target.value })} placeholder="pc1.corp.example or 10.0.0.5" className={field} />
            </div>
            <div className="space-y-1">
              <label className={label}>Port</label>
              <input value={portText} onChange={(e) => setPortText(e.target.value.replace(/[^0-9]/g, ""))} inputMode="numeric" className={field} />
            </div>
          </div>

          <div className="space-y-1">
            <label className={label}>User name</label>
            <input value={draft.username} onChange={(e) => set({ username: e.target.value })} placeholder={"CORP\\me, me@corp.example, or empty to be asked"} className={field} />
          </div>

          <div className="space-y-1">
            <label className={label}>Folder</label>
            <select
              value={draft.folder_id == null ? "" : String(draft.folder_id)}
              onChange={(e) => set({ folder_id: e.target.value ? parseInt(e.target.value, 10) : null })}
              className={field}
            >
              <option value="" className="bg-[#1a1a1e] text-zinc-500">-- Root --</option>
              {folderOptions(folders || []).map((f) => <option key={f.id} value={String(f.id)} className="bg-[#1a1a1e] text-white">{f.label}</option>)}
            </select>
          </div>

          <div className="space-y-1">
            <label className={label}>Notes</label>
            <textarea value={draft.notes} onChange={(e) => set({ notes: e.target.value })} rows={3} className="w-full px-3 py-2 bg-zinc-900/60 border border-white/10 rounded-lg text-[12px] text-zinc-100 outline-none focus:border-primary/50 resize-y" />
          </div>

          <p className="text-[10.5px] text-zinc-400 leading-snug pt-1">
            Opens in Windows' Remote Desktop with your usual Remote Desktop settings. Submarine never stores the
            password: tick “Remember me” in Remote Desktop and Windows keeps it for this computer.
          </p>
        </div>

        <div className="shrink-0 px-5 py-3 border-t border-white/5 flex items-center gap-2">
          <button
            onClick={save}
            disabled={busy}
            className="flex-1 h-10 rounded-lg text-[11.5px] font-bold uppercase tracking-wider bg-primary text-black disabled:opacity-50"
          >
            {busy ? "Saving…" : "Save"}
          </button>
          <button onClick={onClose} className="h-10 px-4 rounded-lg text-[11.5px] font-bold uppercase tracking-wider bg-white/5 text-zinc-200 border border-white/10 hover:bg-white/10">
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
};

export default RdpLinkModal;

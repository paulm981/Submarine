import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, ArrowRight, FolderOpen, FolderSync, GitBranch, Laptop, Loader2, X } from "lucide-react";

// Sync through storage the user owns (src-tauri/src/store_sync.rs): a folder
// that OneDrive, Dropbox, Nextcloud, Syncthing or a network share keeps the
// same on every computer, or a git repository kept to one commit.

export type StoreKind = "folder" | "git";
/// Where a profile syncs to (store_status). A token in a git address is
/// masked here.
export interface StoreInfo { kind: StoreKind; location: string; branch: string; }
/// A computer whose file is in the store (`written`: ms since 1970).
export interface StoreDevice { name: string; written: number; this_one: boolean; }
interface FoundProfile { id: string; name: string; items: number; devices: StoreDevice[]; }

export const DEFAULT_BRANCH = "submarine-sync";

const field = "w-full min-w-0 h-9 px-3 bg-zinc-900/60 border border-white/10 rounded-lg text-[12.5px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600 disabled:opacity-50";

const clean = (e: unknown) => String(e).replace(/^\[[A-Z_]+\]\s*/, "");

export function ago(ms: number): string {
  const s = Math.floor((Date.now() - ms) / 1000);
  if (!ms || s < 0) return "";
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export const storeLabel = (s: StoreInfo) =>
  s.kind === "folder" ? s.location : `${s.location}${s.branch && s.branch !== DEFAULT_BRANCH ? ` · ${s.branch}` : ""}`;

/// The "where": a folder, or a repository and its branch.
export function StoreWhere({
  kind, setKind, location, setLocation, branch, setBranch, disabled,
}: {
  kind: StoreKind; setKind: (k: StoreKind) => void;
  location: string; setLocation: (v: string) => void;
  branch: string; setBranch: (v: string) => void;
  disabled?: boolean;
}) {
  const browse = async () => {
    try {
      const p = await invoke<string | null>("pick_local_directory");
      if (p) setLocation(p);
    } catch { /* no picker here: the path can be typed */ }
  };
  const tab = (k: StoreKind, icon: React.ReactNode, text: string) => (
    <button
      type="button"
      onClick={() => setKind(k)}
      disabled={disabled}
      aria-pressed={kind === k}
      className={`flex-1 h-8 rounded-md text-[11.5px] font-semibold flex items-center justify-center gap-1.5 transition-colors ${
        kind === k ? "bg-primary/15 text-primary" : "text-zinc-400 hover:text-zinc-200"
      }`}
    >
      {icon} {text}
    </button>
  );
  return (
    <div className="space-y-2">
      <div className="flex bg-black/30 border border-white/10 rounded-lg p-0.5">
        {tab("folder", <FolderSync size={12} />, "Synced folder")}
        {tab("git", <GitBranch size={12} />, "Git repository")}
      </div>
      {kind === "folder" ? (
        <>
          <div className="flex gap-1.5">
            <input
              value={location}
              onChange={(e) => setLocation(e.target.value)}
              placeholder="C:\Users\you\OneDrive"
              aria-label="Folder"
              disabled={disabled}
              className={field}
            />
            <button
              type="button"
              onClick={browse}
              disabled={disabled}
              className="h-9 px-3 shrink-0 rounded-lg bg-white/5 border border-white/10 text-zinc-300 hover:text-white text-[12px] flex items-center gap-1.5 disabled:opacity-50"
            >
              <FolderOpen size={12} /> Browse…
            </button>
          </div>
          <p className="text-[11px] text-zinc-500 leading-relaxed">
            A folder that OneDrive, Dropbox, Nextcloud, Syncthing or a network share keeps the same on your
            computers. Submarine keeps its files in a <span className="font-mono">submarine-sync</span> folder inside it.
          </p>
        </>
      ) : (
        <>
          <input
            value={location}
            onChange={(e) => setLocation(e.target.value)}
            placeholder="git@github.com:you/submarine-sync.git"
            aria-label="Repository address"
            disabled={disabled}
            className={`${field} font-mono`}
          />
          <div className="flex items-center gap-2">
            <span className="text-[11px] text-zinc-500 shrink-0">Branch</span>
            <input
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              placeholder={DEFAULT_BRANCH}
              aria-label="Branch"
              disabled={disabled}
              className={`${field} font-mono`}
            />
          </div>
          <p className="text-[11px] text-zinc-500 leading-relaxed">
            A private repository on GitHub, GitLab or your own git server — an empty one is best. The branch is kept
            to a single commit that each sync replaces, so no history piles up. Git signs in the way it does on this
            computer: an SSH key, or Git Credential Manager.
          </p>
        </>
      )}
    </div>
  );
}

/// Set the open profile up to sync through the user's own storage.
export function StoreSetup({ onDone }: { onDone: (msg: string) => void }) {
  const [kind, setKind] = useState<StoreKind>("folder");
  const [location, setLocation] = useState("");
  const [branch, setBranch] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const setup = async () => {
    if (!location.trim()) { setErr(kind === "folder" ? "Choose a folder." : "Enter the repository's address."); return; }
    setBusy(true); setErr(null);
    try {
      const rep = await invoke<{ pushed: number; pulled: number }>("store_setup", {
        kind, location: location.trim(), branch: kind === "git" ? branch.trim() || null : null,
      });
      onDone(rep.pulled > 0
        ? `Syncing — ${rep.pulled} item${rep.pulled === 1 ? "" : "s"} came in from your other computers.`
        : "Syncing. On another computer, choose “From storage” in the profile list to get this profile there.");
    } catch (e) {
      setErr(clean(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3">
      <p className="text-[12px] text-zinc-400 leading-relaxed">
        Keep this profile the same on your computers through storage you own — no third-party service. Everything is
        encrypted with this profile's password before it leaves this computer; without the password, nothing stored
        there can be read.
      </p>
      <StoreWhere kind={kind} setKind={(k) => { setKind(k); setErr(null); }} location={location} setLocation={setLocation} branch={branch} setBranch={setBranch} disabled={busy} />
      {err && (
        <div className="px-3 py-2 rounded-lg bg-rose-500/10 border border-rose-500/30 text-rose-200 text-[11.5px] flex items-start gap-2">
          <AlertTriangle size={12} className="mt-0.5 shrink-0" /> <span className="break-words">{err}</span>
        </div>
      )}
      <button
        onClick={setup}
        disabled={busy}
        className="h-9 px-3 rounded-lg bg-primary text-black text-[12.5px] font-bold disabled:opacity-40 flex items-center gap-2"
      >
        {busy ? <Loader2 size={13} className="animate-spin" /> : <FolderSync size={13} />}
        {busy ? "Setting up…" : "Start syncing"}
      </button>
    </div>
  );
}

/// The computers whose files are in the store.
export function StoreDevices({ devices }: { devices: StoreDevice[] }) {
  if (devices.length === 0) return null;
  return (
    <div className="space-y-0.5">
      {devices.map((d, i) => (
        <div key={i} className="flex items-center gap-2 text-[11.5px]">
          <Laptop size={11} className="text-zinc-500 shrink-0" />
          <span className="text-zinc-300 truncate flex-1">
            {d.name}{d.this_one && <span className="text-zinc-500"> (this computer)</span>}
          </span>
          <span className="text-[10.5px] text-zinc-600 shrink-0">{ago(d.written)}</span>
        </div>
      ))}
    </div>
  );
}

/// The profile list's "From your storage": find this user's profiles in a
/// folder or repository by password, and make one a profile here.
export function StoreRestoreModal({
  existing, onClose, onRestored,
}: {
  existing: string[];
  onClose: () => void;
  onRestored: (name: string, pulled: number) => void;
}) {
  const [kind, setKind] = useState<StoreKind>("folder");
  const [location, setLocation] = useState("");
  const [branch, setBranch] = useState("");
  const [password, setPassword] = useState("");
  const [found, setFound] = useState<FoundProfile[] | null>(null);
  const [pick, setPick] = useState<string>("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState<"" | "find" | "restore">("");
  const [err, setErr] = useState<string | null>(null);
  const nameRef = useRef<HTMLInputElement | null>(null);

  // A name nothing here has yet, from the one the profile has elsewhere.
  const freeName = (base: string) => {
    const taken = new Set(existing.map((n) => n.toLowerCase()));
    const b = base.replace(/[^A-Za-z0-9_-]/g, "-").slice(0, 28) || "profile";
    if (!taken.has(b.toLowerCase())) return b;
    for (let i = 2; i < 100; i++) if (!taken.has(`${b}-${i}`.toLowerCase())) return `${b}-${i}`;
    return b;
  };

  useEffect(() => {
    const f = found?.find((p) => p.id === pick);
    if (f) setName(freeName(f.name));
    // freeName reads `existing`, which doesn't change while this is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pick, found]);

  const where = { kind, location: location.trim(), branch: kind === "git" ? branch.trim() || null : null };

  const find = async () => {
    if (!location.trim()) { setErr(kind === "folder" ? "Choose the folder." : "Enter the repository's address."); return; }
    if (!password) { setErr("Enter the profile's password."); return; }
    setBusy("find"); setErr(null); setFound(null);
    try {
      const list = await invoke<FoundProfile[]>("store_find_profiles", { ...where, password });
      setFound(list);
      setPick(list[0]?.id ?? "");
      requestAnimationFrame(() => nameRef.current?.focus());
    } catch (e) {
      setErr(clean(e));
    } finally {
      setBusy("");
    }
  };

  const restore = async () => {
    const n = name.trim();
    if (!n) { setErr("Pick a name for it here."); return; }
    if (existing.some((x) => x.toLowerCase() === n.toLowerCase())) { setErr(`There's already a profile called "${n}" here.`); return; }
    setBusy("restore"); setErr(null);
    try {
      const rep = await invoke<{ pushed: number; pulled: number }>("store_restore_profile", {
        ...where, profileId: pick, localName: n, password,
      });
      onRestored(n, rep.pulled);
    } catch (e) {
      setErr(clean(e));
    } finally {
      setBusy("");
    }
  };

  const pickedProfile = found?.find((p) => p.id === pick);

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={() => !busy && onClose()}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Get a profile from your storage"
        className="w-full max-w-md max-h-[90vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            <FolderSync size={13} className="text-primary" /> From your storage
          </span>
          <button onClick={onClose} disabled={!!busy} className="text-zinc-400 hover:text-white disabled:opacity-40"><X size={16} /></button>
        </div>
        <div className="flex-1 overflow-y-auto custom-scrollbar p-5 space-y-3">
          <p className="text-[12px] text-zinc-400 leading-relaxed">
            Get a profile you sync from another computer. Point to the same folder or repository, and enter the
            profile's password.
          </p>
          <StoreWhere kind={kind} setKind={(k) => { setKind(k); setFound(null); setErr(null); }} location={location} setLocation={(v) => { setLocation(v); setFound(null); }} branch={branch} setBranch={(v) => { setBranch(v); setFound(null); }} disabled={!!busy} />
          <input
            type="password"
            value={password}
            onChange={(e) => { setPassword(e.target.value); setFound(null); }}
            onKeyDown={(e) => e.key === "Enter" && !found && find()}
            placeholder="The profile's password"
            aria-label="The profile's password"
            disabled={!!busy}
            className={field}
          />
          {err && (
            <div className="px-3 py-2 rounded-lg bg-rose-500/10 border border-rose-500/30 text-rose-200 text-[11.5px] flex items-start gap-2">
              <AlertTriangle size={12} className="mt-0.5 shrink-0" /> <span className="break-words">{err}</span>
            </div>
          )}
          {!found ? (
            <button
              onClick={find}
              disabled={!!busy}
              className="w-full h-9 rounded-lg bg-primary text-black text-[12.5px] font-bold disabled:opacity-40 flex items-center justify-center gap-2"
            >
              {busy === "find" ? <><Loader2 size={13} className="animate-spin" /> Looking…</> : <>Find my profiles <ArrowRight size={13} /></>}
            </button>
          ) : (
            <div className="space-y-2.5 pt-1 border-t border-white/5">
              <div className="space-y-1">
                {found.map((p) => (
                  <label key={p.id} className={`flex items-start gap-2 p-2 rounded-lg border cursor-pointer ${pick === p.id ? "border-primary/40 bg-primary/5" : "border-white/5 hover:bg-white/[0.02]"}`}>
                    <input type="radio" name="store-profile" checked={pick === p.id} onChange={() => setPick(p.id)} className="mt-1 accent-[rgb(var(--primary))]" />
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px] text-zinc-100 truncate">{p.name}</span>
                      <span className="block text-[11px] text-zinc-500">
                        {p.items} item{p.items === 1 ? "" : "s"}
                        {p.devices.length > 0 && <> · from {p.devices.map((d) => d.name).join(", ")}</>}
                        {p.devices[0] && <> · changed {ago(p.devices[0].written)}</>}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
              {pickedProfile && (
                <div className="space-y-1">
                  <span className="text-[10px] font-bold text-zinc-300 uppercase tracking-wider ml-0.5">Name on this computer</span>
                  <input
                    ref={nameRef}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && restore()}
                    aria-label="Name on this computer"
                    disabled={!!busy}
                    className={field}
                  />
                </div>
              )}
              <button
                onClick={restore}
                disabled={!!busy || !pick}
                className="w-full h-9 rounded-lg bg-primary text-black text-[12.5px] font-bold disabled:opacity-40 flex items-center justify-center gap-2"
              >
                {busy === "restore" ? <><Loader2 size={13} className="animate-spin" /> Getting it…</> : <>Get it and open it <ArrowRight size={13} /></>}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

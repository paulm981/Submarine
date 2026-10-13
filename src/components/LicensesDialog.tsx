import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, ChevronRight, ExternalLink, Scale, Search, X } from "lucide-react";
import { SCHEMES } from "../ui/terminalThemes";

// Settings › Open-source licenses: every package that ships in the app with
// its licence texts (public/licenses/third-party.json, written by
// scripts/notices.mjs on each build), and the terminal color schemes with
// theirs (public/licenses/color-schemes/).

interface Pkg {
  name: string;
  version: string;
  license: string;
  url: string | null;
  /** Ids into `texts`; for a color scheme, its licence file instead. */
  texts: string[];
  file?: string;
  by?: string;
}
interface Group { id: string; title: string; packages: Pkg[] }
interface Notices { groups: Group[]; texts: Record<string, string> }

const schemeGroup = (): Group => ({
  id: "schemes",
  title: "Terminal color schemes",
  packages: SCHEMES.filter((s) => s.credit).map((s) => ({
    name: s.name, version: "", license: s.credit!.license, url: s.credit!.url,
    texts: [], file: s.credit!.licenseFile, by: s.credit!.by,
  })),
});

function Row({ pkg, texts }: { pkg: Pkg; texts: Record<string, string> }) {
  const [open, setOpen] = useState(false);
  const [fileText, setFileText] = useState<string | null>(null);
  useEffect(() => {
    if (!open || !pkg.file || fileText !== null) return;
    fetch(`/licenses/color-schemes/${pkg.file}`)
      .then((r) => (r.ok ? r.text() : Promise.reject()))
      .then(setFileText, () => setFileText(""));
  }, [open, pkg.file, fileText]);
  const bodies = pkg.file ? (fileText ? [fileText] : []) : pkg.texts.map((t) => texts[t]).filter(Boolean);
  return (
    <div className="border-b border-white/5 last:border-b-0">
      <button
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-white/[0.03] transition-colors"
      >
        {open ? <ChevronDown size={12} className="text-zinc-500 shrink-0" /> : <ChevronRight size={12} className="text-zinc-500 shrink-0" />}
        <span className="text-[12px] font-semibold text-zinc-200 truncate">{pkg.name}</span>
        {pkg.version && <span className="text-[11px] text-zinc-500 font-mono shrink-0">{pkg.version}</span>}
        <span className="flex-1" />
        <span className="text-[10.5px] text-zinc-400 font-mono truncate max-w-[45%]" title={pkg.license}>{pkg.license}</span>
      </button>
      {open && (
        <div className="px-3 pb-3 pl-8 space-y-2">
          {(pkg.by || pkg.url) && (
            <div className="flex items-center gap-2 text-[11px] text-zinc-500">
              {pkg.by && <span>By {pkg.by}</span>}
              {pkg.url && (
                <button
                  onClick={() => invoke("open_external_url", { url: pkg.url }).catch(() => {})}
                  className="inline-flex items-center gap-1 text-zinc-400 hover:text-white truncate"
                  title={pkg.url}
                >
                  <ExternalLink size={10} className="shrink-0" /> <span className="truncate">{pkg.url}</span>
                </button>
              )}
            </div>
          )}
          {bodies.length === 0 ? (
            <p className="text-[11px] text-zinc-500">{pkg.file && fileText === null ? "Loading…" : `Licensed under ${pkg.license}.`}</p>
          ) : (
            bodies.map((b, i) => (
              <pre key={i} className="m-0 max-h-64 overflow-auto custom-scrollbar whitespace-pre-wrap break-words rounded-lg bg-black/40 border border-white/5 p-2.5 text-[10.5px] leading-relaxed text-zinc-300 font-mono select-text">
                {b}
              </pre>
            ))
          )}
        </div>
      )}
    </div>
  );
}

export default function LicensesDialog({ onClose }: { onClose: () => void }) {
  const [notices, setNotices] = useState<Notices | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  useEffect(() => {
    fetch("/licenses/third-party.json")
      .then((r) => (r.ok ? r.json() : Promise.reject()))
      .then(setNotices, () => setFailed(true));
  }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const groups = useMemo(() => {
    const all = [...(notices?.groups || []), schemeGroup()];
    const q = query.trim().toLowerCase();
    if (!q) return all;
    return all
      .map((g) => ({ ...g, packages: g.packages.filter((p) => `${p.name} ${p.license}`.toLowerCase().includes(q)) }))
      .filter((g) => g.packages.length > 0);
  }, [notices, query]);
  const total = (notices?.groups || []).reduce((n, g) => n + g.packages.length, 0) + schemeGroup().packages.length;

  return createPortal(
    <div className="fixed inset-0 z-[10000] bg-black/70 backdrop-blur-sm flex items-center justify-center p-4" onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="licenses-title"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-[640px] h-[85vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl"
      >
        <div className="flex items-center justify-between gap-3 p-4 border-b border-white/5 shrink-0">
          <div className="flex items-center gap-2 min-w-0">
            <Scale size={14} className="text-primary shrink-0" />
            <h3 id="licenses-title" className="text-[12px] font-black uppercase tracking-widest text-zinc-100">Open-source licenses</h3>
          </div>
          <button
            onClick={onClose}
            aria-label="Close"
            className="w-7 h-7 flex items-center justify-center text-zinc-500 hover:text-white bg-white/[0.04] border border-white/10 hover:bg-white/10 rounded-lg transition-all"
          >
            <X size={14} />
          </button>
        </div>
        <div className="px-4 pt-3 pb-2 space-y-2 shrink-0">
          <p className="text-[12px] text-zinc-400 leading-relaxed">
            Submarine is built on open-source software. These are the {total} packages and color schemes it ships
            with, each with its licence.
          </p>
          <div className="flex items-center gap-2 h-8 px-2.5 rounded-lg bg-black/40 border border-white/10 focus-within:border-primary/50">
            <Search size={12} className="text-zinc-500 shrink-0" />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search by name or licence"
              aria-label="Search licenses"
              className="flex-1 min-w-0 bg-transparent outline-none text-[12px] text-zinc-100 placeholder:text-zinc-600"
            />
          </div>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar px-4 pb-4 space-y-3">
          {failed && (
            <p className="text-[12px] text-amber-300/90">
              The package list is written when the app is built (npm run build), and this copy has none. The
              color schemes are below.
            </p>
          )}
          {!notices && !failed && <p className="text-[12px] text-zinc-500">Loading…</p>}
          {groups.map((g) => (
            <div key={g.id}>
              <div className="text-[10.5px] font-black text-zinc-500 uppercase tracking-wider mb-1">
                {g.title} <span className="text-zinc-600">· {g.packages.length}</span>
              </div>
              <div className="rounded-lg border border-white/5 bg-black/20">
                {g.packages.map((p) => <Row key={`${p.name}@${p.version}`} pkg={p} texts={notices?.texts || {}} />)}
              </div>
            </div>
          ))}
          {query && groups.length === 0 && <p className="text-[12px] text-zinc-500">Nothing matches "{query}".</p>}
        </div>
      </div>
    </div>,
    document.body,
  );
}

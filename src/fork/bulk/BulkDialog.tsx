import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowDownToLine, ArrowUpFromLine, Check, File, Folder, FolderOpen, Loader2, X } from "lucide-react";
import { useBroadcast } from "../../ui/broadcast";
import { localJoin } from "../../fs/localPath";
import { IS_LINUX } from "../../util/platform";
import {
  AT_ONCE, type Check as ServerCheck, type Choice, type LocalCount, type LocalEntry, type LocalPick, type Mode, PROGRESS_MS,
  type Row, type Step, countLocal, failureOf, finalRow, folderNames, isStepEvent, localBase, overwriteFor, percent, pullLine,
  pullStep, pushLine, pushSteps, remoteDirArg, remoteItemArg, rowText, runFew, runTotals, summarize, thereLine, throttle,
} from "./plan";
import { canBulk, hideBulk, setBulkRunning, useBulkAsk } from "./store";

// ─────────────────────────────────────────────────────────────────────────────
// Bulk push and pull (#159): files from this computer to many servers, or a
// file or folder from each of them into a folder of its own here, over the
// app's own SFTP transfers, a few servers at a time. Always mounted (from
// DesktopApp, beside the ping panel), so a run goes on while it's hidden.
//
// Setup → a check of every server → a summary (with one choice about what's
// already there) → the run, one row per server.
// ─────────────────────────────────────────────────────────────────────────────

type Session = { id: string; serverName: string; protocol?: string };
type Phase = "setup" | "checking" | "review" | "running" | "finished";
type Listing = { current_path: string; entries: LocalEntry[] };
type RemoteListing = { current_path: string; entries: { name: string; is_dir: boolean }[] };
type Progress = { id?: string; name?: string; kind?: string; bytes?: number; total?: number; status?: string; error?: string | null };

/** One server in a run: its session, its name, and what its check found. */
interface Target { sessionId: string; name: string; folder?: string; check: ServerCheck }

const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
/** Names on this computer: Windows and macOS don't tell `A` from `a`. */
const sameName = IS_LINUX ? (s: string) => s : (s: string) => s.toLowerCase();
/** Rows of a folder listed for picking; past this, pick a smaller folder. */
const LIST_MAX = 1000;

const btn = "h-8 px-3 rounded-lg text-[12px] font-semibold border transition-colors disabled:opacity-40 disabled:cursor-default";
const btnPlain = `${btn} border-white/10 bg-white/[0.04] text-zinc-200 hover:bg-white/[0.08]`;
const btnMain = `${btn} border-primary/40 bg-primary/20 text-primary hover:bg-primary/30`;
const input = "w-full h-8 px-2.5 bg-zinc-900/60 border border-white/10 rounded-lg text-[12px] text-zinc-50 outline-none focus:border-primary/50 placeholder:text-zinc-600 font-mono";
const label = "text-[10px] font-bold uppercase tracking-widest text-zinc-500";

export const BulkTransfer = ({ sessions, statuses }: { sessions: Session[]; statuses: Record<string, string> }) => {
  const ask = useBulkAsk();
  const broadcast = useBroadcast();
  const usable = useMemo(() => sessions.filter((s) => canBulk(s, statuses[s.id])), [sessions, statuses]);

  const [phase, setPhase] = useState<Phase>("setup");
  const [mode, setMode] = useState<Mode>("push");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  // Push: a folder here, and what in it to send (or the folder itself).
  const [src, setSrc] = useState<{ dir: string; entries: LocalEntry[] } | null>(null);
  const [whole, setWhole] = useState(false);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [remoteDir, setRemoteDir] = useState("~");
  // Pull: a path on the servers, and the folder here.
  const [remoteItem, setRemoteItem] = useState("");
  const [localDir, setLocalDir] = useState<string | null>(null);
  // The check, the summary and the run.
  const [targets, setTargets] = useState<Target[]>([]);
  const [count, setCount] = useState<LocalCount | null>(null);
  const [choice, setChoice] = useState<Choice>("skip");
  const [rows, setRows] = useState<Record<string, Row>>({});
  const [stopping, setStopping] = useState(false);
  const rowsRef = useRef<Record<string, Row>>({});
  const stopRef = useRef<{ stopped: boolean; active: Map<string, { step: Step; ids: Set<string>; note?: string }> }>({
    stopped: false, active: new Map(),
  });
  const checkSeq = useRef(0);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const shownRef = useRef(false);
  shownRef.current = !!ask;
  // A run that ended while hidden: its rows wait to be seen.
  const unseenRef = useRef(false);

  // A new ask starts over, unless a run is going or a check is: then the
  // dialog just shows again.
  useEffect(() => {
    if (!ask) return;
    if (phaseRef.current === "running" || phaseRef.current === "checking") return;
    if (phaseRef.current === "finished" && unseenRef.current) {
      unseenRef.current = false;
      setBulkRunning(false);
      return;
    }
    const b = broadcast.stateRef.current;
    const ok = new Set(usable.map((s) => s.id));
    const fromBroadcast = b.enabled ? [...b.targetSessionIds].filter((id) => ok.has(id)) : [];
    setPicked(new Set(fromBroadcast.length ? fromBroadcast : ask.from.filter((id) => ok.has(id))));
    setMode(ask.mode);
    setPhase("setup");
    setError(null);
    setTargets([]);
    setRows({});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask?.seq]);

  // Esc hides it; a run goes on.
  useEffect(() => {
    if (!ask) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") hideBulk(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ask]);

  const poke = useMemo(() => throttle(() => setRows({ ...rowsRef.current }), PROGRESS_MS), []);
  const setRow = (sid: string, r: Row) => { rowsRef.current[sid] = r; poke(); };

  // ── Setup ──────────────────────────────────────────────────────────────────

  const chooseSource = async () => {
    setError(null);
    try {
      const dir = await invoke<string | null>("select_local_folder");
      if (!dir) return;
      const listing = await invoke<Listing>("local_list_dir", { path: dir });
      setSrc({ dir: listing.current_path, entries: listing.entries });
      setWhole(false);
      setTicked(new Set());
    } catch (e) { setError(errText(e)); }
  };

  const chooseLocalDir = async () => {
    setError(null);
    try {
      const dir = await invoke<string | null>("select_local_folder");
      if (dir) setLocalDir(dir);
    } catch (e) { setError(errText(e)); }
  };

  const picks: (LocalPick & { size: number })[] = useMemo(() => {
    if (!src) return [];
    if (whole) {
      const name = localBase(src.dir);
      return name ? [{ path: src.dir, name, isDir: true, size: 0 }] : [];
    }
    return src.entries
      .filter((e) => ticked.has(e.path) && !e.is_symlink)
      .map((e) => ({ path: e.path, name: e.name, isDir: e.is_dir, size: e.size }));
  }, [src, whole, ticked]);

  const servers = usable.filter((s) => picked.has(s.id));
  const ready = servers.length > 0 && (mode === "push" ? picks.length > 0 && remoteDir.trim() !== "" : remoteItem.trim() !== "" && !!localDir);

  // ── The check ──────────────────────────────────────────────────────────────

  const check = async () => {
    setError(null);
    const list = servers.map((s) => ({ sessionId: s.id, name: s.serverName }));
    const seq = ++checkSeq.current;
    if (mode === "push") {
      const dirArg = remoteDirArg(remoteDir);
      if (dirArg === null) { setError("Type a folder on the servers that starts with / or ~ (your home folder there)."); return; }
      setPhase("checking");
      const found: Target[] = list.map((t) => ({ ...t, check: { ok: false, reason: "Not checked" } }));
      const counting = countLocal(picks, (path) => invoke<Listing>("local_list_dir", { path }).then((l) => l.entries));
      await runFew(found, AT_ONCE, async (t) => {
        try {
          const l = await invoke<RemoteListing>("sftp_list_dir", { sessionId: t.sessionId, path: dirArg });
          t.check = { ok: true, steps: pushSteps(picks, l.current_path, new Set(l.entries.map((e) => e.name))) };
        } catch (e) {
          t.check = { ok: false, reason: `Can't open ${remoteDir.trim()} there: ${errText(e)}` };
        }
      }, () => checkSeq.current !== seq);
      const c = await counting;
      if (checkSeq.current !== seq) return;
      setCount(c);
      setTargets(found);
    } else {
      const item = remoteItemArg(remoteItem);
      if (item === null) { setError("Type the full path of a file or folder on the servers, starting with / or ~/."); return; }
      if (!localDir) return;
      setPhase("checking");
      const folders = folderNames(list.map((t) => t.name));
      const found: Target[] = list.map((t, i) => ({ ...t, folder: localJoin(localDir, folders[i]), check: { ok: false, reason: "Not checked" } }));
      // What's here already: the folders in the one picked, and what's in
      // each server's own folder, listed once each.
      let here: Set<string>;
      try {
        here = new Set((await invoke<Listing>("local_list_dir", { path: localDir })).entries.map((e) => sameName(e.name)));
      } catch (e) {
        setPhase("setup");
        setError(`Can't open ${localDir}: ${errText(e)}`);
        return;
      }
      await runFew(found, AT_ONCE, async (t, i) => {
        let entry: { name: string; is_dir: boolean } | undefined;
        try {
          const l = await invoke<RemoteListing>("sftp_list_dir", { sessionId: t.sessionId, path: item.dir });
          entry = l.entries.find((e) => e.name === item.name);
          if (!entry) { t.check = { ok: false, reason: `${remoteItem.trim()} isn't there` }; return; }
          let there = false;
          if (here.has(sameName(folders[i]))) {
            const mine = await invoke<Listing>("local_list_dir", { path: t.folder! }).catch(() => null);
            there = !!mine?.entries.some((e) => sameName(e.name) === sameName(item.name));
          }
          t.check = { ok: true, steps: [pullStep(l.current_path, item.name, entry.is_dir, t.folder!, there)] };
        } catch (e) {
          t.check = { ok: false, reason: `Can't look there: ${errText(e)}` };
        }
      }, () => checkSeq.current !== seq);
      if (checkSeq.current !== seq) return;
      setCount(null);
      setTargets(found);
    }
    setChoice("skip");
    setPhase("review");
  };

  // ── The run ────────────────────────────────────────────────────────────────

  const start = async () => {
    const run = { stopped: false, active: new Map<string, { step: Step; ids: Set<string>; note?: string }>() };
    stopRef.current = run;
    setStopping(false);
    const runChoice = choice;
    rowsRef.current = Object.fromEntries(targets.map((t) => [t.sessionId, t.check.ok ? { state: "queued" } : { state: "failed", reason: t.check.reason }] as [string, Row]));
    setRows({ ...rowsRef.current });
    setPhase("running");
    setBulkRunning(true);

    // Each server's progress: the transfer it's on says so by its kind and
    // name, and gives its id, which is what Stop cancels.
    const unlisten = await Promise.all(targets.filter((t) => t.check.ok).map((t) =>
      listen<Progress>(`sftp-transfer-${t.sessionId}`, (ev) => {
        const p = ev.payload;
        const a = run.active.get(t.sessionId);
        if (!p?.id || !a || !isStepEvent(a.step, p)) return;
        if (!a.ids.has(p.id)) {
          a.ids.add(p.id);
          if (run.stopped) invoke("sftp_cancel_transfer", { transferId: p.id }).catch(() => {});
        }
        if (p.status === "done" && p.error) a.note = p.error;
        const r = rowsRef.current[t.sessionId];
        if (p.status === "progress" && r?.state === "copying") setRow(t.sessionId, { ...r, pct: percent(p.bytes ?? 0, p.total ?? 0) });
      }).catch(() => () => {}),
    ));

    try {
      await runFew(targets, AT_ONCE, async (t) => {
        if (!t.check.ok) return;
        const steps = t.check.steps;
        let copied = 0;
        let skipped = 0;
        const failed: string[] = [];
        const notes: string[] = [];
        let folderMade = false;
        for (let i = 0; i < steps.length; i++) {
          if (run.stopped) { setRow(t.sessionId, { state: "stopped" }); return; }
          const step = steps[i];
          const overwrite = overwriteFor(step, runChoice);
          if (overwrite === null) { skipped++; continue; }
          setRow(t.sessionId, { state: "copying", step: i + 1, steps: steps.length, pct: null });
          const a = { step, ids: new Set<string>(), note: undefined as string | undefined };
          run.active.set(t.sessionId, a);
          try {
            // A pull's own folder here, made when something goes in it.
            if (t.folder && !folderMade) { await invoke("local_create_dir", { path: t.folder }); folderMade = true; }
            await invoke(step.cmd, { sessionId: t.sessionId, localPath: step.localPath, remotePath: step.remotePath, overwrite });
            copied++;
            if (a.note) notes.push(a.note);
          } catch (e) {
            const f = failureOf(e);
            if (f.end === "skipped") skipped++;
            else if (f.end === "stopped" || run.stopped) { setRow(t.sessionId, { state: "stopped" }); return; }
            else failed.push(steps.length > 1 ? `${step.name}: ${f.reason}` : f.reason);
          } finally {
            run.active.delete(t.sessionId);
          }
        }
        if (failed.length) {
          const more = failed.length > 1 ? ` (and ${failed.length - 1} more)` : "";
          setRow(t.sessionId, { state: "failed", reason: `${failed[0]}${more}${copied ? `. ${copied} of ${steps.length} copied` : ""}` });
        } else {
          setRow(t.sessionId, finalRow(copied, skipped, notes.join(" ")));
        }
      }, () => run.stopped);
    } finally {
      for (const u of unlisten) u();
      for (const t of targets) {
        if (rowsRef.current[t.sessionId]?.state === "queued") rowsRef.current[t.sessionId] = { state: "stopped" };
      }
      setRows({ ...rowsRef.current });
      setPhase("finished");
      // Hidden: the menus offer to show how it went until it's been seen.
      unseenRef.current = !shownRef.current;
      setBulkRunning(unseenRef.current);
    }
  };

  /** Stop: nothing more starts, and what's copying is cancelled the way its
   *  card's Cancel would. */
  const cancelAll = () => {
    const run = stopRef.current;
    run.stopped = true;
    for (const a of run.active.values()) {
      for (const id of a.ids) invoke("sftp_cancel_transfer", { transferId: id }).catch(() => {});
    }
  };
  const stop = () => { setStopping(true); cancelAll(); };
  // Gone with the window's sessions (the profile locked): so is the run.
  useEffect(() => () => { cancelAll(); setBulkRunning(false); }, []);

  if (!ask) return null;

  // ── The dialog ─────────────────────────────────────────────────────────────

  const summary = summarize(targets.map((t) => t.check));
  const busy = phase === "running" || phase === "checking";
  const title = mode === "push" ? "Push files to servers" : "Pull files from servers";
  const dirShown = remoteDir.trim() || "~";

  return (
    <div className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-3" onClick={() => { if (!busy) hideBulk(); }}>
      <div role="dialog" aria-modal="true" aria-label={title} className="w-full max-w-xl max-h-[85vh] flex flex-col bg-[#121214] border border-white/10 rounded-xl shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="shrink-0 px-4 py-3 border-b border-white/5 flex items-center justify-between gap-3">
          <span className="text-[12px] font-bold uppercase tracking-widest text-white flex items-center gap-2">
            {mode === "push" ? <ArrowUpFromLine size={13} className="text-indigo-300" /> : <ArrowDownToLine size={13} className="text-indigo-300" />}
            {title}
          </span>
          <button onClick={hideBulk} title={busy ? "Hide: it goes on copying" : "Close"} aria-label="Close" className="text-zinc-400 hover:text-white"><X size={16} /></button>
        </div>

        <div className="flex-1 overflow-y-auto custom-scrollbar p-4 space-y-4 text-[12px] text-zinc-300">
          {phase === "setup" && (
            <>
              <div className="flex gap-1 p-0.5 rounded-lg bg-white/[0.04] border border-white/10 w-fit">
                {(["push", "pull"] as const).map((m) => (
                  <button
                    key={m}
                    onClick={() => setMode(m)}
                    className={`h-7 px-3 rounded-md text-[11.5px] font-semibold ${mode === m ? "bg-primary/20 text-primary" : "text-zinc-400 hover:text-white"}`}
                  >{m === "push" ? "Push: this computer to servers" : "Pull: servers to this computer"}</button>
                ))}
              </div>

              <section className="space-y-1.5">
                <div className="flex items-center justify-between">
                  <span className={label}>Servers ({servers.length} of {usable.length})</span>
                  {usable.length > 1 && (
                    <span className="flex gap-2 text-[11px]">
                      <button className="text-zinc-400 hover:text-white" onClick={() => setPicked(new Set(usable.map((s) => s.id)))}>All</button>
                      <button className="text-zinc-400 hover:text-white" onClick={() => setPicked(new Set())}>None</button>
                    </span>
                  )}
                </div>
                {usable.length === 0 ? (
                  <p className="text-zinc-500 italic">No SSH session is connected. Open the servers first.</p>
                ) : (
                  <div className="max-h-40 overflow-y-auto custom-scrollbar rounded-lg border border-white/10 p-1">
                    {usable.map((s) => (
                      <label key={s.id} className="flex items-center gap-2 px-2 py-1 rounded hover:bg-white/[0.04] cursor-pointer">
                        <input
                          type="checkbox"
                          className="accent-primary"
                          checked={picked.has(s.id)}
                          onChange={() => setPicked((prev) => {
                            const next = new Set(prev);
                            if (next.has(s.id)) next.delete(s.id); else next.add(s.id);
                            return next;
                          })}
                        />
                        <span className="truncate">{s.serverName}</span>
                      </label>
                    ))}
                  </div>
                )}
              </section>

              {mode === "push" ? (
                <>
                  <section className="space-y-1.5">
                    <span className={label}>What to send</span>
                    <div className="flex items-center gap-2">
                      <button className={btnPlain} onClick={() => void chooseSource()}><FolderOpen size={12} className="inline mr-1.5 -mt-0.5" />Choose a folder…</button>
                      {src && <span className="truncate font-mono text-[11px] text-zinc-400" title={src.dir}>{src.dir}</span>}
                    </div>
                    {src && (
                      <div className="max-h-48 overflow-y-auto custom-scrollbar rounded-lg border border-white/10 p-1">
                        {localBase(src.dir) && (
                          <label className="flex items-center gap-2 px-2 py-1 rounded hover:bg-white/[0.04] cursor-pointer">
                            <input type="checkbox" className="accent-primary" checked={whole} onChange={() => setWhole((w) => !w)} />
                            <Folder size={12} className="text-indigo-300 shrink-0" />
                            <span className="truncate">{localBase(src.dir)} <span className="text-zinc-500">(the whole folder)</span></span>
                          </label>
                        )}
                        {src.entries.slice(0, LIST_MAX).map((e) => (
                          <label key={e.path} className={`flex items-center gap-2 px-2 py-1 rounded ${whole || e.is_symlink ? "opacity-40" : "hover:bg-white/[0.04] cursor-pointer"}`} title={e.is_symlink ? "A link: links aren't sent" : undefined}>
                            <input
                              type="checkbox"
                              className="accent-primary"
                              disabled={whole || !!e.is_symlink}
                              checked={!whole && ticked.has(e.path)}
                              onChange={() => setTicked((prev) => {
                                const next = new Set(prev);
                                if (next.has(e.path)) next.delete(e.path); else next.add(e.path);
                                return next;
                              })}
                            />
                            {e.is_dir ? <Folder size={12} className="text-indigo-300 shrink-0" /> : <File size={12} className="text-zinc-500 shrink-0" />}
                            <span className="truncate">{e.name}</span>
                          </label>
                        ))}
                        {src.entries.length > LIST_MAX && (
                          <p className="px-2 py-1 text-zinc-500 italic">And {(src.entries.length - LIST_MAX).toLocaleString()} more. Choose a folder with fewer in it, or send the whole folder.</p>
                        )}
                        {src.entries.length === 0 && <p className="px-2 py-1 text-zinc-500 italic">This folder is empty.</p>}
                      </div>
                    )}
                  </section>
                  <section className="space-y-1.5">
                    <span className={label}>Into this folder on each server</span>
                    <input className={input} value={remoteDir} onChange={(e) => setRemoteDir(e.target.value)} placeholder="~ or /path/to/folder" spellCheck={false} />
                    <p className="text-[11px] text-zinc-500">~ is your home folder on each server.</p>
                  </section>
                </>
              ) : (
                <>
                  <section className="space-y-1.5">
                    <span className={label}>File or folder on each server</span>
                    <input className={input} value={remoteItem} onChange={(e) => setRemoteItem(e.target.value)} placeholder="~/logs/app.log or /var/log/nginx" spellCheck={false} />
                  </section>
                  <section className="space-y-1.5">
                    <span className={label}>Into this folder here</span>
                    <div className="flex items-center gap-2">
                      <button className={btnPlain} onClick={() => void chooseLocalDir()}><FolderOpen size={12} className="inline mr-1.5 -mt-0.5" />Choose a folder…</button>
                      {localDir && <span className="truncate font-mono text-[11px] text-zinc-400" title={localDir}>{localDir}</span>}
                    </div>
                    <p className="text-[11px] text-zinc-500">Each server's copy goes in a folder of its own, named after the server.</p>
                  </section>
                </>
              )}
            </>
          )}

          {phase === "checking" && (
            <p className="flex items-center gap-2 text-zinc-400"><Loader2 size={13} className="animate-spin" /> Checking {servers.length === 1 ? "the server" : `${servers.length} servers`}…</p>
          )}

          {phase === "review" && (
            <>
              <p className="text-zinc-200">
                {mode === "push"
                  ? `Send ${pushLine(count ?? { files: 0, bytes: 0, more: false }, summary.servers, dirShown)}`
                  : `Fetch ${pullLine(remoteItem.trim(), summary.servers, localDir ?? "")}`}
              </p>
              {summary.problems > 0 && (
                <div className="space-y-1">
                  <p className="text-amber-300">{summary.problems === 1 ? "1 server is left out:" : `${summary.problems} servers are left out:`}</p>
                  <ul className="space-y-0.5 pl-3 text-[11.5px] text-zinc-400">
                    {targets.filter((t) => !t.check.ok).map((t) => (
                      <li key={t.sessionId}><span className="text-zinc-200">{t.name}</span>: {!t.check.ok && t.check.reason}</li>
                    ))}
                  </ul>
                </div>
              )}
              {summary.there > 0 && (
                <fieldset className="space-y-1.5">
                  <legend className="text-zinc-200 mb-1">{thereLine(summary, mode)}</legend>
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input type="radio" className="accent-primary" checked={choice === "skip"} onChange={() => setChoice("skip")} />
                    Skip those: leave them as they are
                  </label>
                  <label className="flex items-center gap-2 cursor-pointer">
                    <input type="radio" className="accent-primary" checked={choice === "replace"} onChange={() => setChoice("replace")} />
                    Replace them
                  </label>
                </fieldset>
              )}
              {summary.servers === 0 && <p className="text-zinc-400">There's nothing to copy.</p>}
            </>
          )}

          {(phase === "running" || phase === "finished") && (
            <>
              {phase === "finished" && <p className="text-zinc-200">{runTotals(targets.map((t) => rows[t.sessionId] ?? { state: "stopped" }))}</p>}
              <ul className="space-y-1">
                {targets.map((t) => {
                  const r = rows[t.sessionId] ?? { state: "queued" };
                  const tone = r.state === "failed" ? "text-rose-300" : r.state === "done" ? "text-emerald-300" : r.state === "copying" ? "text-indigo-200" : "text-zinc-400";
                  return (
                    <li key={t.sessionId} className="px-2 py-1.5 rounded-lg border border-white/5 bg-white/[0.02]">
                      <div className="flex items-center gap-2">
                        {r.state === "copying" ? <Loader2 size={12} className="animate-spin text-indigo-300 shrink-0" />
                          : r.state === "done" ? <Check size={12} className="text-emerald-400 shrink-0" />
                          : <span className="w-3 shrink-0" />}
                        <span className="truncate text-zinc-200 min-w-0 flex-1">{t.name}</span>
                        <span className={`text-[11px] ${tone} text-right break-words max-w-[65%]`}>{rowText(r)}</span>
                      </div>
                      {r.state === "copying" && r.pct !== null && (
                        <div className="mt-1 h-1 rounded bg-white/5 overflow-hidden"><div className="h-full bg-indigo-500" style={{ width: `${r.pct}%` }} /></div>
                      )}
                    </li>
                  );
                })}
              </ul>
            </>
          )}

          {error && <p className="text-rose-300">{error}</p>}
        </div>

        <div className="shrink-0 px-4 py-3 border-t border-white/5 flex items-center justify-end gap-2">
          {phase === "setup" && (
            <>
              <button className={btnPlain} onClick={hideBulk}>Cancel</button>
              <button className={btnMain} disabled={!ready} onClick={() => void check()}>Next</button>
            </>
          )}
          {phase === "checking" && <button className={btnPlain} onClick={() => { checkSeq.current++; setPhase("setup"); }}>Back</button>}
          {phase === "review" && (
            <>
              <button className={btnPlain} onClick={() => setPhase("setup")}>Back</button>
              <button className={btnMain} disabled={summary.servers === 0} onClick={() => void start()}>Start</button>
            </>
          )}
          {phase === "running" && (
            <>
              <button className={btnPlain} onClick={hideBulk}>Hide</button>
              <button className={`${btn} border-rose-500/40 bg-rose-500/15 text-rose-300 hover:bg-rose-500/25`} disabled={stopping} onClick={stop}>{stopping ? "Stopping…" : "Stop"}</button>
            </>
          )}
          {phase === "finished" && (
            <>
              {mode === "pull" && localDir && (
                <button className={btnPlain} onClick={() => { invoke("local_open_in_explorer", { localPath: localDir }).catch((e) => setError(errText(e))); }}>Show the folder</button>
              )}
              <button className={btnPlain} onClick={() => setPhase("setup")}>New copy</button>
              <button className={btnMain} onClick={hideBulk}>Close</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};

export default BulkTransfer;

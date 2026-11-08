import { AlertTriangle, CheckCircle2, FolderSync, GitBranch, Loader2, RefreshCw, ShieldOff } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { StoreDevices, StoreSetup, storeLabel } from "../../components/StoreSync";
import type { ConfirmOptions } from "../../ui/confirm";
import type { StoreStats, StoreSync } from "./hooks";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's pieces of the Profile panel, each rendered by ProfilePanel in one
// line (#163): ProfilePanel.tsx is the original project's file. Each renders
// what ProfilePanel did in its place, or nothing where it didn't.
// ─────────────────────────────────────────────────────────────────────────────

type SyncDiff = { needs_push: number; needs_pull: number; out_of_sync_nodes: string[] };

/** ProfilePanel's sync controls and state, as the store section reads them. */
type SyncProps = {
  stats: (StoreStats & { diff: SyncDiff | null }) | null;
  syncing: boolean;
  lastSyncLabel: string | null;
  autoSync: boolean;
  syncIntervalMin: number;
  onSetInterval: (m: number) => void;
  onToggleAutoSync: () => void;
  doSync: () => Promise<void>;
};

/** Sync now, auto-sync and how this computer compares, for the user's own
 *  storage: the original project's controls in the Submarine Cloud section,
 *  with `partner` naming where it syncs to in place of "the cloud". Replacing
 *  the cloud copy isn't offered here: it's the cloud's. */
const SyncControls = ({ partner, stats, syncing, lastSyncLabel, autoSync, syncIntervalMin, onSetInterval, onToggleAutoSync, doSync }: SyncProps & { partner: string }) => (
  <>
    <div className="flex items-center gap-2 flex-wrap">
      <button
        onClick={doSync}
        disabled={syncing}
        className="h-9 px-3 rounded-lg bg-primary text-black text-[12.5px] font-bold disabled:opacity-40 flex items-center gap-2"
      >
        <RefreshCw size={13} className={syncing ? "animate-spin" : ""} />
        {syncing ? "Syncing…" : "Sync now"}
      </button>
      <span className="text-[11px] text-zinc-500">{lastSyncLabel ?? "Not synced yet this session."}</span>
    </div>
    <div className="flex items-center gap-2 flex-wrap text-[11.5px]">
      {autoSync ? (
        <>
          <span className="flex items-center gap-1.5 text-emerald-300/90">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse-brief" />
            Auto-sync is on — your changes are saved to {partner} automatically.
          </span>
          <button onClick={onToggleAutoSync} className="text-zinc-400 hover:text-zinc-200 underline underline-offset-2 shrink-0">Turn off</button>
        </>
      ) : (
        <>
          <span className="flex items-center gap-1.5 text-zinc-400">
            <span className="w-1.5 h-1.5 rounded-full bg-zinc-500" />
            Auto-sync is off — changes sync only when you press Sync now.
          </span>
          <button onClick={onToggleAutoSync} className="text-primary hover:opacity-80 underline underline-offset-2 shrink-0">Turn on</button>
        </>
      )}
    </div>
    {/* The cadence lives here rather than in Settings: it only means
        anything next to the Sync-now button and the on/off state it
        modifies, and it's the answer to "is this thing even running?" */}
    {autoSync && (
      <div className="border-t border-white/5 pt-3 flex items-center gap-2 flex-wrap">
        <span className="text-[11.5px] text-zinc-400">Check for others' changes every</span>
        <div className="flex items-center gap-1">
          {[1, 2, 5, 10, 15, 30].map((m) => (
            <button
              key={m}
              onClick={() => onSetInterval(m)}
              className={`h-7 px-2.5 rounded-md text-[11.5px] font-medium transition-colors ${
                syncIntervalMin === m
                  ? "bg-primary text-black"
                  : "bg-zinc-900/60 text-zinc-400 hover:text-zinc-200 border border-white/5"
              }`}
            >
              {m}m
            </button>
          ))}
        </div>
      </div>
    )}
    {stats?.diff && (() => {
      const d = stats.diff!;
      const pending = d.needs_push + d.needs_pull;
      if (pending === 0) {
        return (
          <div className="flex items-center gap-1.5 text-[11.5px] text-emerald-300/90">
            <CheckCircle2 size={12} /> Everything is in sync with {partner}.
          </div>
        );
      }
      return (
        <div className="text-[11.5px] text-amber-200/90 space-y-1">
          <div className="flex items-center gap-1.5">
            <AlertTriangle size={12} />
            {d.needs_push > 0 && <span>{d.needs_push} to send</span>}
            {d.needs_push > 0 && d.needs_pull > 0 && <span className="text-zinc-500">·</span>}
            {d.needs_pull > 0 && <span>{d.needs_pull} to receive</span>}
            <span className="text-zinc-500">— run Sync now.</span>
          </div>
          {d.out_of_sync_nodes.length > 0 && (
            <div className="text-[10.5px] text-zinc-500 pl-[18px]">
              Nodes: {d.out_of_sync_nodes.slice(0, 8).join(", ")}
              {d.out_of_sync_nodes.length > 8 ? ` +${d.out_of_sync_nodes.length - 8} more` : ""}
            </div>
          )}
        </div>
      );
    })()}
  </>
);

/** The Sync section, above Submarine Cloud's: where this profile syncs
 *  through the user's own storage, with its controls, the computers syncing
 *  it and Stop syncing; or, when it doesn't, StoreSetup to start (or a word
 *  on Submarine Cloud, for a shared profile). */
export const StoreSection = ({ own, shared, card, label, busy, setBusy, confirm, refresh, loadStats, flash, fail, ...sync }: SyncProps & {
  own: StoreSync;
  shared: boolean;
  card: string;
  label: string;
  busy: boolean;
  setBusy: (busy: boolean) => void;
  confirm: (opts: ConfirmOptions | string) => Promise<boolean>;
  refresh: () => Promise<void>;
  loadStats: () => Promise<void>;
  flash: (m: string) => void;
  fail: (e: unknown) => void;
}) => {
  const { store, cloudOn } = own;
  const { stats, syncing, doSync } = sync;
  const stopStore = async () => {
    if (!(await confirm({
      title: "Stop syncing this profile?",
      message: "It stays on this computer as it is, and what's in your storage stays there — your other computers keep syncing with each other. You can start again any time.",
      okLabel: "Stop syncing",
      destructive: true,
    }))) return;
    setBusy(true);
    try {
      await invoke("store_disconnect");
      await refresh();
      loadStats();
      flash("This profile no longer syncs from this computer.");
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  return (
    <section className={card}>
      <div className={label}>Sync</div>
      {store === undefined ? (
        <div className="text-[11.5px] text-zinc-500 flex items-center gap-1.5"><Loader2 size={11} className="animate-spin" /> Loading…</div>
      ) : store ? (
        <>
          <div className="flex items-center gap-2 min-w-0">
            {store.kind === "folder"
              ? <FolderSync size={14} className="text-primary shrink-0" />
              : <GitBranch size={14} className="text-primary shrink-0" />}
            <span className="text-[12.5px] text-zinc-200 font-mono truncate select-text" title={storeLabel(store)}>{storeLabel(store)}</span>
          </div>
          <SyncControls partner="your storage" {...sync} />
          {stats && stats.unreadable > 0 && (
            <div className="flex items-start gap-1.5 text-[11px] text-amber-300/80">
              <AlertTriangle size={11} className="mt-0.5 shrink-0" />
              <span>
                {stats.unreadable} file{stats.unreadable === 1 ? "" : "s"} in this profile's folder there {stats.unreadable === 1 ? "doesn't" : "don't"} open
                with its key, so {stats.unreadable === 1 ? "it's" : "they're"} left as {stats.unreadable === 1 ? "it is" : "they are"}.
              </span>
            </div>
          )}
          {stats && stats.devices.length > 0 && (
            <div className="border-t border-white/5 pt-3 space-y-1.5">
              <div className="text-[10.5px] text-zinc-500">Computers syncing this profile</div>
              <StoreDevices devices={stats.devices} />
            </div>
          )}
          <div className="pt-2 border-t border-white/5">
            <button
              onClick={stopStore}
              disabled={busy || syncing}
              className="h-8 px-3 rounded-lg bg-white/5 border border-white/10 text-zinc-300 hover:text-rose-300 hover:bg-rose-500/10 text-[11.5px] font-bold flex items-center gap-1.5 disabled:opacity-40"
            >
              <ShieldOff size={12} /> Stop syncing
            </button>
          </div>
        </>
      ) : shared ? (
        <p className="text-[12px] text-zinc-400">
          {cloudOn
            ? "This profile is shared through Submarine Cloud, which keeps it in sync."
            : "This profile is shared through Submarine Cloud, which is turned off — turn it on under Settings › Sync to keep it in sync, or to stop sharing it."}
        </p>
      ) : (
        // Synced once already; this runs the app's own sync too, so its lists
        // show what came in and "Last synced" is right.
        <StoreSetup onDone={(m) => { flash(m); doSync(); }} />
      )}
    </section>
  );
};

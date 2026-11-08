import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { StoreDevice, StoreInfo } from "../../components/StoreSync";

// ─────────────────────────────────────────────────────────────────────────────
// The fork's state in the Profile panel, as hooks ProfilePanel calls in one
// line each (#163): ProfilePanel.tsx is the original project's file, so every
// line the fork adds there can conflict at the next sync.
// ─────────────────────────────────────────────────────────────────────────────

/** What `profile_sync_stats` adds for the user's own storage: the computers
 *  whose files are there, and how many files this profile's key doesn't
 *  open. */
export interface StoreStats {
  devices: StoreDevice[];
  unreadable: number;
}

/** Syncing through the user's own storage (StoreSync), and whether Submarine
 *  Cloud is turned on in Settings, which it's hidden without. `store` is
 *  undefined until known; `read` asks for both again (ProfilePanel's
 *  refresh). ProfilePanel shows its Submarine Cloud and Access sections only
 *  when the cloud is on and the profile doesn't sync through the user's own
 *  storage: sharing goes through the cloud. */
export const useStoreSync = () => {
  const [store, setStore] = useState<StoreInfo | null | undefined>(undefined);
  const [cloudOn, setCloudOn] = useState(false);
  const read = useCallback(() => {
    invoke<StoreInfo | null>("store_status").then(setStore).catch(() => setStore(null));
    invoke<boolean>("get_cloud_enabled").then(setCloudOn).catch(() => setCloudOn(false));
  }, []);
  return { store, cloudOn, read };
};
export type StoreSync = ReturnType<typeof useStoreSync>;

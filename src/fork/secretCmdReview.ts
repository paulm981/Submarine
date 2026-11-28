import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";

export interface SecretCommandMeta {
  id?: number;
  changed_by_other?: boolean;
  edited_by?: string | null;
  updated_at?: string | null;
}

export interface SecretReviewInfo {
  id?: number;
  changedByOther: boolean;
  editedBy?: string | null;
  updatedAt?: string | null;
}

/** Extract review metadata for a Secret Library command changed by someone else (#286). */
export function secretReviewOf(c: SecretCommandMeta): SecretReviewInfo | undefined {
  if (!c.changed_by_other) return undefined;
  return {
    ...(typeof c.id === "number" ? { id: c.id } : {}),
    changedByOther: true,
    editedBy: c.edited_by ?? null,
    updatedAt: c.updated_at ?? null,
  };
}

/** Parse `{phys_ms}:{counter}:{node}` HLC timestamp into a readable local date/time. */
export function formatHlcTime(stamp?: string | null): string | null {
  if (!stamp) return null;
  const msStr = stamp.split(":")[0];
  if (!msStr || !/^\d+$/.test(msStr)) return null;
  const ms = Number(msStr);
  if (!Number.isFinite(ms) || ms <= 0) return null;
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return null;
  return d.toLocaleString();
}

/** Attribution line shown in the run confirmation box for a teammate-edited secret (#286). */
export function secretChangeNotice(review?: SecretReviewInfo): string | undefined {
  if (!review?.changedByOther) return undefined;
  const who = review.editedBy?.trim() || null;
  const when = formatHlcTime(review.updatedAt);
  if (who && when) {
    return t("Last changed by {who} ({when}). Inspect before running.", { who, when });
  }
  if (who) {
    return t("Last changed by {who}. Inspect before running.", { who });
  }
  if (when) {
    return t("Changed by another member via sync ({when}). Inspect before running.", { when });
  }
  return t("Changed by another member via sync. Inspect before running.");
}

/** Extra ConfirmOptions fields forcing reveal before Run for #286. */
export function secretConfirmExtras(review?: SecretReviewInfo): { revealBeforeOk?: boolean; changedNotice?: string } {
  if (!review?.changedByOther) return {};
  return {
    revealBeforeOk: true,
    changedNotice: secretChangeNotice(review),
  };
}

/** True while OK/Run must remain disabled until Show command is pressed (#286). */
export function isConfirmOkLocked(opts: { revealBeforeOk?: boolean }, revealed: boolean): boolean {
  return !!(opts.revealBeforeOk && !revealed);
}

/** Clear the device-local sync review flag after running a revealed command (#304). */
export async function markSecretCommandReviewed(
  review: SecretReviewInfo | undefined,
  ranText: string
): Promise<boolean> {
  if (!review?.changedByOther || typeof review.id !== "number" || !ranText) return false;
  try {
    return await invoke<boolean>("secret_cmd_reviewed", { id: review.id, ranText });
  } catch {
    return false;
  }
}

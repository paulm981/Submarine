// A secret Library command someone else changed (#286): in the box that asks
// before it runs, Run stays greyed until Show command, and a line says who
// changed it and when. Your own secret commands, and every command that isn't
// secret, run as they did.
//
// Who changed it is the backend's to say (src-tauri/src/secret_look.rs): a
// device-local list of the commands whose text a sync brought from another
// account or a device that isn't signed in. A run after Show command takes it
// off the list; the next change from someone else puts it back.
//
// The original project's files reach this in one-line hooks (#163), checked
// by tools/test/fork-hooks.test.ts:
//   - CmdsPanel, the session panel's Run: `lookFirst` wraps its confirm;
//   - ui/confirm: the box's `look` option, `LookLine`, `lookRunProps` on
//     Run, and `lookLocked` in its close, so nothing answers yes while Run
//     is greyed (Enter, a click, a script).
//
// The name comes from another person's app: untrusted. It's bounded here as
// in the backend, and React shows it as text, never as HTML.
import { invoke } from "@tauri-apps/api/core";
import type { ConfirmOptions } from "../../ui/confirm";

export interface SecretLook {
  /** "changed": someone else changed it. "stale": the list's copy isn't
   *  what's saved now. "unknown": the backend couldn't be asked. */
  why: "changed" | "stale" | "unknown";
  changedBy: string | null;
  /** Milliseconds since 1970. */
  changedAt: number | null;
}

/** The most of a name the box shows, in characters (as secret_look.rs). */
export const NAME_MAX = 80;

// Control characters, and the invisible ones that could make a name read as
// someone else's (direction overrides, zero-width marks).
const INVISIBLE = /[\u0000-\u001f\u007f-\u009f­؜᠎​-‏‪-‮⁠-⁯﻿]/g;

/** A name as the box may show it: plain, trimmed, at most NAME_MAX
 *  characters. Null when nothing is left, or it isn't a string. */
export function boundName(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const clean = raw.slice(0, 4 * NAME_MAX).replace(INVISIBLE, "").trim();
  if (!clean) return null;
  const chars = Array.from(clean);
  return chars.length <= NAME_MAX ? clean : `${chars.slice(0, NAME_MAX - 1).join("")}…`;
}

/** What the backend said, checked: null when a run needn't wait. */
export function parseLook(raw: unknown): SecretLook | null {
  if (raw === null || raw === undefined) return null;
  const r = (typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const why = r.why === "changed" || r.why === "stale" ? r.why : "unknown";
  const at = typeof r.changedAt === "number" && Number.isFinite(r.changedAt) && r.changedAt > 0 ? r.changedAt : null;
  return { why, changedBy: boundName(r.changedBy), changedAt: at };
}

/** The line the box shows above the command (without its full stop). */
export function lookLine(look: SecretLook, date: (ms: number) => string = (ms) => new Date(ms).toLocaleString()): string {
  if (look.why === "stale") return "Changed since this list was loaded";
  if (look.why === "unknown") return "Submarine couldn't check who last changed it";
  const when = look.changedAt ? date(look.changedAt) : null;
  if (look.changedBy) return when ? `Changed by ${look.changedBy} on ${when}` : `Changed by ${look.changedBy}`;
  return when ? `Changed on another device on ${when}` : "Changed on another device";
}

/** Whether Run is greyed: a box with a look whose command isn't shown. */
export const lookLocked = (opts: ConfirmOptions | null | undefined, revealed: boolean): boolean =>
  !!opts?.look && !!opts.revealableText && !revealed;

/** Run's extra props in the box: greyed while `lookLocked`. */
export const lookRunProps = (opts: ConfirmOptions, revealed: boolean) =>
  lookLocked(opts, revealed)
    ? { disabled: true, title: "Show the command first", style: { opacity: 0.4, cursor: "not-allowed" } }
    : {};

/** Who changed it, and that Run waits for Show command. Text only. */
export const LookLine = ({ look }: { look?: SecretLook }) =>
  look ? (
    <p data-secret-look className="mb-2 text-[11px] leading-relaxed text-amber-300 break-words">
      {lookLine(look)}. <span className="text-zinc-400">Show the command to check it before you run it.</span>
    </p>
  ) : null;

/**
 * `confirm` for the Library's Run of command `id`, whose text the list holds
 * as `content`. Only a secret's box (the one with `revealableText`) asks the
 * backend first; any other passes straight through, as before. A run after
 * Show command, of a command that had a look, is told to the backend, which
 * takes it off its list while the text is still the saved one. If the
 * backend can't be asked, Run waits for Show command all the same.
 */
export function lookFirst(
  confirm: (opts: ConfirmOptions) => Promise<boolean>,
  id: number,
  content: string,
  ask: typeof invoke = invoke,
): (opts: ConfirmOptions) => Promise<boolean> {
  return async (opts) => {
    if (!opts.revealableText) return confirm(opts);
    let look: SecretLook | null;
    try {
      look = parseLook(await ask("secret_look_get", { id, content }));
    } catch {
      look = { why: "unknown", changedBy: null, changedAt: null };
    }
    if (!look) return confirm(opts);
    const ok = await confirm({ ...opts, look });
    // Not waited for: the run goes at once, and saving the vault can take a moment.
    if (ok) void ask("secret_look_seen", { id, content }).catch(() => undefined);
    return ok;
  };
}

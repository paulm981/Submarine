import { invoke } from "@tauri-apps/api/core";

// Feedback in this fork (#236, decision 3): the original project's form sends
// what you type to its author's server (src-tauri/src/feedback.rs), which
// can't see the fork's issues and isn't one the fork otherwise talks to. Here
// every way to Feedback (Settings, the profile list, Suggest a fix under
// Language) opens a new issue on the fork's GitHub in the browser instead:
// nothing is sent until you submit it there, and what you write is public.

/** A new issue on the fork's GitHub. */
export const FORK_NEW_ISSUE_URL = "https://github.com/paulm981/Submarine/issues/new";

/** The new-issue page, its text started with `prefill` (Suggest a fix: the
 *  language). */
export const feedbackIssueUrl = (prefill?: string): string =>
  prefill && prefill.trim() ? `${FORK_NEW_ISSUE_URL}?body=${encodeURIComponent(`${prefill.trim()}\n\n`)}` : FORK_NEW_ISSUE_URL;

/** Opens Feedback the fork's way, in place of the original project's form.
 *  Returns true: the form isn't shown. */
export function feedbackOnGithub(opts: { prefill?: string } = {}): boolean {
  invoke("open_external_url", { url: feedbackIssueUrl(opts.prefill) }).catch((e) => console.error("[feedback] couldn't open the issue page:", e));
  return true;
}

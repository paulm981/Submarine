import { invoke } from "@tauri-apps/api/core";
import { copyText } from "../../util/clipboard";

// Feedback in this fork (#236, decision 3): the original project's form sends
// what you type to its author's server (src-tauri/src/feedback.rs), which
// can't see the fork's issues and isn't one the fork otherwise talks to. Here
// every way to Feedback (Settings, the profile list, Suggest a fix under
// Language, Report this reply in the AI chat) opens a new issue on the fork's
// GitHub in the browser instead: nothing is sent until you submit it there,
// and what you write is public.

/** A new issue on the fork's GitHub. */
export const FORK_NEW_ISSUE_URL = "https://github.com/paulm981/Submarine/issues/new";

/** The new-issue page, its text started with `prefill` (Suggest a fix: the
 *  language). */
export const feedbackIssueUrl = (prefill?: string): string =>
  prefill && prefill.trim() ? `${FORK_NEW_ISSUE_URL}?body=${encodeURIComponent(`${prefill.trim()}\n\n`)}` : FORK_NEW_ISSUE_URL;

/** What "Report this reply?" says in the fork (spread over the original
 *  project's words in ChatPanel), since the reply goes to the clipboard and
 *  the fork's issue page, not into the in-app form. Typed as optional so it
 *  can be spread after the original's own words and win. */
export const REPORT_REPLY_CONFIRM: { message?: string; okLabel?: string } = {
  message:
    "The reply is copied to your clipboard, and a new issue opens on this fork's GitHub in your browser: paste it there and say what's wrong with it. " +
    "Nothing is sent until you submit it there, and what you post is public, so take out anything private from your servers. " +
    "If it can't be copied, select it in the chat and copy it.",
  okLabel: "Open the issue page",
};

/** Copies `append` (Report this reply: the reply), then opens the new-issue
 *  page. The reply goes to the clipboard, never into the page's address: the
 *  browser sends that to GitHub as soon as the page opens, before the person
 *  decides to submit, and a reply can hold details of their servers. */
export async function openOnGithub(opts: { prefill?: string; append?: string } = {}): Promise<void> {
  if (opts.append) {
    // Before the browser opens, while the app still has the focus a copy needs.
    try { await copyText(opts.append); } catch (e) { console.error("[feedback] couldn't copy the reply:", e); }
  }
  try { await invoke("open_external_url", { url: feedbackIssueUrl(opts.prefill) }); } catch (e) { console.error("[feedback] couldn't open the issue page:", e); }
}

/** Opens Feedback the fork's way, in place of the original project's form.
 *  Returns true: the form isn't shown. */
export function feedbackOnGithub(opts: { prefill?: string; append?: string } = {}): boolean {
  void openOnGithub(opts);
  return true;
}

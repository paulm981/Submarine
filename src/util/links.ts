/** Where the privacy policy is published. The original project links its
 *  website's page; the fork links its own PRIVACY.md, which says where the
 *  fork differs (Submarine Cloud off unless turned on, sync through your own
 *  storage, the fork's update check). */
export const PRIVACY_URL = "https://github.com/paulm981/Submarine/blob/main/PRIVACY.md";

/** A new GitHub issue, for when the feedback inbox is full for the day. */
export const REPO_ISSUES_URL = "https://github.com/SinaXhpm/Submarine/issues/new";

/** A new GitHub issue to fix a translation, its title naming the language
 *  ("Translation (de): "), with a short form to fill in. */
export function translationIssueUrl(language: string): string {
  const title = `Translation (${language}): `;
  const body = "Where in the app:\n\nIt says now:\n\nBetter:\n\nWhy (optional):\n";
  return `https://github.com/SinaXhpm/Submarine/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

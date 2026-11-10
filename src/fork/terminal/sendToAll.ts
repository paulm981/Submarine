// Send to all (#218): a set of commands typed into every Broadcast server,
// line by line, as if typed there. For what a paste is awkward for, like
// putting the same key into sixty servers' authorized_keys.
import { withoutPasteControls } from "../../util/pasteText";

/** The most that's sent at once: a set of commands, not a file. */
export const MAX_SEND = 256 * 1024;

export type Prepared =
  | { ok: true; data: number[]; lines: number; cleaned: boolean }
  | { ok: false; why: string };

/** Lines in `text`, as they'll be sent (a last line break doesn't start one). */
export const lineCount = (text: string): number =>
  text ? text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n").length : 0;

/** What's typed for `raw`: control characters left out (escape sequences,
 *  Ctrl+C and the like never get through), each line break an Enter, and
 *  one after the last line when `enter`. */
export const prepareSend = (raw: string, enter: boolean): Prepared => {
  if (raw.length > MAX_SEND) return { ok: false, why: `That's more than ${MAX_SEND / 1024} KB. Send a set of commands, not a file.` };
  const text = withoutPasteControls(raw);
  const cleaned = text !== raw;
  if (!text.trim()) return { ok: false, why: cleaned ? "Nothing to send: it was only control characters." : "Nothing to send." };
  let body = text.replace(/\r\n?/g, "\n");
  if (enter && !body.endsWith("\n")) body += "\n";
  const data = Array.from(new TextEncoder().encode(body.replace(/\n/g, "\r")));
  return { ok: true, data, lines: lineCount(body), cleaned };
};

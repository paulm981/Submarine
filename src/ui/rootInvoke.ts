import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTextPrompt } from "./confirm";

// What needs root on a server — an Info panel action (a service, a kill,
// firewall rules), a Docker command where the user isn't in the docker
// group — runs through sudo or dzdo (run_as_root in lib.rs). When the tool
// wants a password it's asked for here; the app keeps what worked for that
// tab only, in memory, and also uses what the tab's SFTP root mode has.
const ROOT_MESSAGES: Record<string, string> = {
  "[SUDO] NEED_PASSWORD": "sudo / dzdo needs a password.",
  "[SUDO] WRONG_PASSWORD": "Wrong sudo / dzdo password.",
  "[SUDO] NOT_ALLOWED": "This user isn't allowed to run that with sudo or dzdo.",
  "[SUDO] REQUIRETTY": "sudo on this server requires a terminal (requiretty), so the panel can't use it. Run the command in the terminal instead.",
  "[SUDO] NO_SUDO": "This needs root, and neither sudo nor dzdo is installed on this server.",
};

export const describeRootError = (raw: string) => {
  const code = Object.keys(ROOT_MESSAGES).find((k) => raw.includes(k));
  return code ? ROOT_MESSAGES[code] : raw.replace(/^\[SUDO\] FAILED:?\s*/, "sudo failed: ");
};

// The password being asked for, per tab (`sessionId`): calls that find they
// need it meanwhile (a list and its details opening together) wait for the
// same answer instead of asking again.
const asking = new Map<string, Promise<string | null>>();

/// invoke() for a command that may run as root: called without a password
/// first; when sudo or dzdo asks for one, the user is asked and the command
/// is called again with it. Null when the user cancels. Errors come back as
/// Error with a readable message.
export const useRootInvoke = () => {
  const textPrompt = useTextPrompt();
  return useCallback(async <T,>(cmd: string, args: Record<string, unknown>, what: string): Promise<T | null> => {
    const tab = String(args.sessionId ?? "");
    let password: string | null = null;
    for (let wrongTries = 0; ; ) {
      try {
        return await invoke<T>(cmd, { ...args, password });
      } catch (e: any) {
        const raw = typeof e === "string" ? e : (e?.message || String(e));
        const wrong = raw.includes("[SUDO] WRONG_PASSWORD");
        if ((!wrong && !raw.includes("[SUDO] NEED_PASSWORD")) || (wrong && ++wrongTries >= 3)) {
          throw new Error(describeRootError(raw));
        }
        let answer = asking.get(tab);
        if (!answer) {
          answer = textPrompt({
            title: `${what} as root`,
            message: wrong
              ? "Wrong password, try again."
              : "This needs root, and sudo or dzdo on this server wants your password. It's kept in memory for this tab only and never saved.",
            password: true,
            keepWhitespace: true,
            okLabel: "Run as root",
            validate: (v) => (v ? null : "Enter the password"),
          });
          asking.set(tab, answer);
          const mine = answer;
          void mine.finally(() => { if (asking.get(tab) === mine) asking.delete(tab); });
        }
        password = await answer;
        if (password === null) return null;
      }
    }
  }, [textPrompt]);
};

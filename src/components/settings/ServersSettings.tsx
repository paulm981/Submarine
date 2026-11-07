// Settings › Servers & sessions: how servers show and close, the jump host
// queue, and sessions kept between connections.
import { useEffect, useState } from "react";
import { History, Server } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { KeepSessionSetting } from "../KeptSessions";
import { CARD, LearnMore, SettingsSection, SwitchRow } from "./parts";

// How many sessions log in to the same jump host at once (multiexec.rs):
// opening a folder of servers behind one queues the rest. Kept by the
// backend, which does the queueing.
function JumpLoginLimit() {
  const [limit, setLimit] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<number>("get_jump_login_limit").then((n) => { setLimit(n); setDraft(String(n)); }).catch(() => setLimit(null));
  }, []);
  const save = () => {
    const n = Number(draft.trim());
    if (!Number.isInteger(n) || n < 0 || n > 256) {
      setError("A whole number from 0 to 256.");
      return;
    }
    setError(null);
    invoke<number>("set_jump_login_limit", { limit: n })
      .then((v) => { setLimit(v); setDraft(String(v)); })
      .catch((e) => setError(String(e)));
  };
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="text-sm font-semibold text-zinc-100">Logins at a time per jump host</div>
        <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
          How many servers behind one jump host log in at once; the rest wait their turn. 0 means no limit.
        </p>
        <LearnMore>
          When many servers behind one jump host open at once (a whole folder opened, say), this many log in to it
          together and the rest wait their turn — a jump host turns away logins past its own limit (OpenSSH starts
          at 10). Each jump host in a ProxyJump chain from ~/.ssh/config has its own line too: shared with a saved
          jump host at the same address, or, behind another jump host, its own for that way there. Servers reached
          directly aren't held back.
        </LearnMore>
        {error && <p className="text-[11px] text-red-400 mt-1">{error}</p>}
      </div>
      <input
        type="number"
        min={0}
        max={256}
        aria-label="Logins at a time per jump host"
        value={draft}
        disabled={limit === null}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => { if (e.key === "Enter") save(); }}
        className="shrink-0 w-20 h-9 px-2 bg-zinc-900/60 border border-white/10 rounded-lg text-[13px] text-zinc-50 text-center outline-none focus:border-primary/50"
      />
    </div>
  );
}

const mono = "font-mono text-zinc-300";

export function ServersSection({ settings, setSettings }: any) {
  return (
    <SettingsSection icon={Server} title="Servers">
      <div className={CARD}>
        <SwitchRow
          title="Show each server's OS"
          line="A badge beside each server's name says which OS it runs."
          more={
            <p>
              The first time a server connects, Submarine reads <span className={mono}>/etc/os-release</span>{" "}
              (or <span className={mono}>uname</span> / <span className={mono}>ver</span>) in the background for the
              badge. Turn off where every command is recorded or a key only allows one command.
            </p>
          }
          on={settings.detectOs !== false}
          onToggle={() => setSettings((s: any) => ({ ...s, detectOs: !(s.detectOs !== false) }))}
        />
        <SwitchRow
          title="Ask before closing a connected session"
          line="Closing a tab whose SSH connection is up asks first. Off, it just closes."
          more="Closing several sessions at once, or quitting with sessions open, still asks."
          on={settings.confirmCloseSession !== false}
          onToggle={() => setSettings((s: any) => ({ ...s, confirmCloseSession: !(s.confirmCloseSession !== false) }))}
        />
        <SwitchRow
          title="Play a sound when a pinged server answers again"
          line="A short, quiet tone when a server you're pinging (right-click it, then Ping) can be logged in to again, after a reboot, say."
          on={settings.pingSound !== false}
          onToggle={() => setSettings((s: any) => ({ ...s, pingSound: !(s.pingSound !== false) }))}
        />
        <JumpLoginLimit />
      </div>
    </SettingsSection>
  );
}

export function SessionsSection() {
  return (
    <SettingsSection icon={History} title="Sessions">
      <KeepSessionSetting />
    </SettingsSection>
  );
}

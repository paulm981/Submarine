// Settings › Servers & sessions: how servers show and close, jump hosts (how
// many logins, and whether tabs share them), and sessions kept between
// connections.
import { useEffect, useState } from "react";
import { History, Network, Server } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { KeepSessionSetting } from "../KeptSessions";
import { CARD, LearnMore, SettingsSection, SwitchRow } from "./parts";

// The "Jump hosts" card: one number and one switch. The number is
// "Logins per jump host" (multiexec.rs, jump_login_limit.json): with sharing
// off, how many tabs log in to a jump host at once; with sharing on, also how
// many logins one jump host keeps open for the tabs that share them
// (jump_pool.rs, #237). The switch is jump_share.json, off unless turned on.
// Both are kept by the backend, which does the queueing and the sharing.
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
        <div className="text-sm font-semibold text-zinc-100">Logins per jump host</div>
        <p className="text-[12px] text-zinc-400 leading-relaxed mt-1">
          How many logins to one jump host there can be at a time. 0 means no limit.
        </p>
        <LearnMore>
          <p>
            With sharing off, each tab logs in to the jump host itself, and this many log in at once: when you open a
            whole folder, the rest wait their turn. A jump host turns away logins past its own limit (OpenSSH starts at
            10).
          </p>
          <p className="mt-2">
            With sharing on, it's also how many logins to one jump host stay open for the tabs that share them. There's
            one at first. Another is made only when every login already has two tabs, and a new tab goes on the login
            with the fewest. With 0, there's a login for every two tabs.
          </p>
          <p className="mt-2">
            Each jump host in a ProxyJump chain from ~/.ssh/config counts on its own: as the same machine as a saved
            jump host at the same address, or, behind another jump host, on its own for that way there. Servers reached
            directly aren't held back.
          </p>
        </LearnMore>
        {error && <p className="text-[11px] text-red-400 mt-1">{error}</p>}
      </div>
      <input
        type="number"
        min={0}
        max={256}
        aria-label="Logins per jump host"
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

function JumpSharing() {
  const [on, setOn] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<boolean>("get_jump_sharing").then(setOn).catch(() => setOn(null));
  }, []);
  const toggle = () => {
    if (on === null) return;
    invoke<boolean>("set_jump_sharing", { on: !on })
      .then((v) => { setOn(v); setError(null); })
      .catch((e) => setError(String(e)));
  };
  return (
    <SwitchRow
      title="Share logins between tabs"
      line="SSH tabs behind the same jump host log in to it once and share that login, up to the number above."
      more={
        <>
          <p>
            A one-time code or a push approval is asked once for each login, not once for each tab. Opening a big
            folder can still ask up to the number above times, once as each new login is made. A password you typed
            is used again for those; a code isn't.
          </p>
          <p className="mt-2">
            Big output, such as a large file printed to the screen, can be slower through a shared login than through
            a tab's own, and a tab flooding output can slow typing in the other tabs on the same login a little.
          </p>
          <p className="mt-2">
            Tabs only share when they log in as the same user, with the same password or key, the same way to the jump
            host. A tab's own SFTP and port forwarding connections, a terminal's own login, Remote Desktop, VNC, telnet,
            FTP and Monitoring keep logins of their own. A login stays open 30 seconds after its last tab closes. A jump
            host that records sessions sees one login for all the tabs on it.
          </p>
          <p className="mt-2">Off, each tab logs in itself. Tabs already sharing a login keep it until they reconnect.</p>
        </>
      }
      on={on === true}
      disabled={on === null}
      onToggle={toggle}
      error={error}
    />
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
      </div>
    </SettingsSection>
  );
}

export function JumpHostsSection() {
  return (
    <SettingsSection icon={Network} title="Jump hosts">
      <div className={CARD}>
        <JumpLoginLimit />
        <JumpSharing />
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

// Settings › AI tools (#32, src-tauri/src/ai): off by default, on this
// device only. On, Home's menu lists the AI tools installed (Claude Code,
// Codex: #117) under AI tools, started already connected to Submarine; off,
// nothing listens and every AI tab's access ends. The folders the AI may
// copy files into (#159) are kept here too: added only with the system's
// folder picker, and no tool of the AI's can change them.
import { useEffect, useState } from "react";
import { Bot, FolderPlus, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { AiTrustOverview } from "../AiTrust";
import { LearnMore, SettingsSection, SwitchRow } from "./parts";

interface AiToolInfo { id: string; label: string; path: string | null }
interface AiSettings { enabled: boolean; flash: boolean; tools: AiToolInfo[]; pull_folders: string[]; available: boolean }

// Claude Code's own settings that keep it out of your keys: deny rules for
// its Read tool, and the sandbox's `credentials` for the commands it runs
// (code.claude.com/docs/en/permissions and /sandboxing).
const KEY_FOLDERS = ["~/.ssh", "~/.aws", "~/.config/gcloud", "~/.azure"];
const CLAUDE_KEYS_OUT = JSON.stringify({
  permissions: { deny: KEY_FOLDERS.map((f) => `Read(${f}/**)`) },
  sandbox: { enabled: true, credentials: { files: KEY_FOLDERS.map((path) => ({ path, mode: "deny" })) } },
}, null, 2);

function AiToolsSettings() {
  const [info, setInfo] = useState<AiSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<AiSettings>("ai_settings").then(setInfo).catch((e) => setError(String(e)));
  }, []);
  const toggle = () => {
    if (!info) return;
    setError(null);
    invoke<AiSettings>("ai_set_enabled", { enabled: !info.enabled }).then(setInfo).catch((e) => setError(String(e)));
  };
  const toggleFlash = () => {
    if (!info) return;
    setError(null);
    invoke<AiSettings>("ai_set_flash", { flash: !info.flash }).then(setInfo).catch((e) => setError(String(e)));
  };
  const addFolder = () => {
    setError(null);
    invoke<AiSettings>("ai_add_pull_folder").then(setInfo).catch((e) => setError(String(e)));
  };
  const removeFolder = (path: string) => {
    setError(null);
    invoke<AiSettings>("ai_remove_pull_folder", { path }).then(setInfo).catch((e) => setError(String(e)));
  };
  if (!info) return error ? <p className="text-[11.5px] text-red-400">{error}</p> : null;
  return (
    <div className="bg-[#121215] border border-white/5 rounded-2xl p-6 space-y-3 shadow-xl">
      <SwitchRow
        title="AI tools"
        line="Let an AI tool you start from Home run commands on the server tabs you share with it, after you allow them."
        more={
          <>
            <p>
              It never gets a key or a password, and never connects to a server itself: Submarine runs each command
              on its own connection, after you click Allow in the AI panel, or at once where you've said it may (below).
              Each AI tab only reaches the tabs shared with it, and closing it ends its access.
            </p>
            <p>
              When it asks to run something as root, Submarine runs it through the server's dzdo or sudo (dzdo su - where
              that's the way in), and asks you on the request for your own password for the server's login, if dzdo or
              sudo wants it. The AI never sees it. The password is only
              as safe as the server account, though: a program running as that account could catch it. A sudoers rule
              that needs no password for the commands you want is safer.
            </p>
          </>
        }
        on={info.enabled}
        onToggle={toggle}
      />
      {info.enabled && (
        <div className="text-[12px] text-amber-200/90 leading-relaxed rounded-lg border border-amber-400/20 bg-amber-400/[0.06] px-3 py-2">
          <p>Output you allow goes to the AI provider, and so do files the AI tool reads on this computer.</p>
          <LearnMore>
            <p className="text-amber-200/80">
              Each command's output, with the secrets Submarine recognises hidden, is sent to the AI tool and on to its
              provider: for Claude Code, Anthropic, or the cloud you've set it to use; for Codex, OpenAI.
            </p>
          </LearnMore>
        </div>
      )}
      <ul className="text-[12px] text-zinc-400 leading-relaxed space-y-1">
        {info.tools.map((t) => (
          <li key={t.id}>
            {t.path ? (
              <><span className="text-zinc-200">{t.label}</span> is installed: <span className="font-mono text-[11.5px] text-zinc-300 break-all select-text">{t.path}</span></>
            ) : (
              <><span className="text-zinc-200">{t.label}</span> isn't installed on this computer.</>
            )}
          </li>
        ))}
      </ul>
      <div>
        <p className="text-[12px] text-zinc-400 leading-relaxed">
          {info.tools.some((t) => t.path)
            ? (info.enabled ? "Start one from Home: the arrow beside +, then AI tools." : "Turn this on to find them in Home's menu.")
            : "Install one, then open this again."}
        </p>
        <LearnMore label="What the AI can see">
          <p>
            Each tool starts already connected to Submarine, and doesn't ask a second time before it uses Submarine's
            tools: Submarine asks you before each command itself.
          </p>
          <p>
            What the AI gets back is filtered: private keys, password and token values, password hashes, access keys and
            passwords in URLs are replaced with [hidden by Submarine]. The filter catches common shapes, not everything.
            Every request and decision is logged on this computer, in ai-audit.log in the log folder (Help ›
            Troubleshooting).
          </p>
          <p>
            The AI tool starts in a folder of its own, without your SSH agent and the usual cloud logins (AWS, Azure, Google
            Cloud, GitHub) in its environment. But it runs as you, so ssh run by it can still log in with an agent named by
            IdentityAgent in ~/.ssh/config (1Password, Secretive, Bitwarden, Pageant), a connection your own terminal keeps
            open (ControlMaster), or a key file with no passphrase or with its passphrase in the macOS Keychain. What it runs
            can also read your files and use the network.
          </p>
          <p>
            To keep Claude Code out of your keys, add this to ~/.claude/settings.json. The deny rules stop its own Read tool;
            the sandbox stops the commands it runs, which by default can read these folders too. The sandbox runs on macOS,
            Linux and WSL2, not on Windows itself: for production servers on Windows, run Submarine and its AI tools in WSL2
            or a virtual machine.
          </p>
          <pre className="whitespace-pre-wrap break-all rounded-lg bg-black/40 border border-white/5 p-2 font-mono text-[10.5px] text-zinc-400 select-text">{CLAUDE_KEYS_OUT}</pre>
        </LearnMore>
      </div>
      <div className="pt-3 border-t border-white/5">
        <SwitchRow
          small
          title="Flash the taskbar when the AI needs you"
          line="When a command waits for your Allow and Submarine isn't in front, its taskbar button flashes. Nothing else moves or takes the keyboard."
          on={info.flash}
          onToggle={toggleFlash}
        />
      </div>
      {info.available && (
        <div className="pt-3 border-t border-white/5 space-y-2">
          <div className="text-sm font-semibold text-zinc-100">Folders the AI may copy into</div>
          <p className="text-[11.5px] text-zinc-500 leading-relaxed">
            When the AI copies files from your servers to this computer, they go only into one of these folders, each
            server's in a folder of its own, and never replace a file that's there. None to start with: until you add one,
            the AI can't copy anything here.
          </p>
          <LearnMore>
            A folder of its own is best, such as one for AI downloads, rather than your home folder. Copies to your servers
            can come from anywhere on this computer, as you allow each one on its card. Kept on this computer only; the AI
            can't change this list.
          </LearnMore>
          {info.pull_folders.length > 0 && (
            <ul className="space-y-1">
              {info.pull_folders.map((f) => (
                <li key={f} className="flex items-center gap-2 rounded-lg border border-white/5 bg-black/20 px-2 py-1">
                  <span className="flex-1 font-mono text-[11.5px] text-zinc-300 break-all select-text">{f}</span>
                  <button
                    type="button"
                    onClick={() => removeFolder(f)}
                    title="Remove this folder"
                    aria-label={`Remove ${f}`}
                    className="shrink-0 p-1 rounded text-zinc-500 hover:text-red-300 hover:bg-white/5"
                  >
                    <X size={13} />
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button
            type="button"
            onClick={addFolder}
            className="h-8 px-3 rounded-lg border border-white/10 bg-white/[0.03] text-[12px] text-zinc-200 hover:border-primary/40 hover:text-white flex items-center gap-1.5"
          >
            <FolderPlus size={13} /> Add a folder…
          </button>
        </div>
      )}
      <div className="pt-3 border-t border-white/5 space-y-2">
        <div className="text-sm font-semibold text-zinc-100">How much the AI may run without asking</div>
        <p className="text-[11.5px] text-zinc-500 leading-relaxed">
          Kept on this computer only, never synced. Whatever you choose, every command is in the AI panel and the
          audit log, and Stop ends it.
        </p>
        <LearnMore>
          A shared profile can't change it, and the AI can't either. A profile file exported or copied to another
          computer arrives asking every time, with only its always-ask list.
        </LearnMore>
        <AiTrustOverview />
      </div>
      {error && <p className="text-[11.5px] text-red-400">{error}</p>}
    </div>
  );
}

export function AiToolsSection() {
  return (
    <SettingsSection icon={Bot} title="AI Tools">
      <AiToolsSettings />
    </SettingsSection>
  );
}

// Canned answers for the app's backend calls, so the real page runs in a
// browser for screenshots (take.mjs). Loaded before src/main.tsx. Every
// server, user and address here is made up: the addresses are from the
// documentation ranges 192.0.2.0/24, 198.51.100.0/24 and 203.0.113.0/24.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

mockWindows("main");

const now = Date.now();
const ago = (mins) => new Date(now - mins * 60000).toISOString();

const folders = [
  { id: 1, name: "Production", parent_id: null, color: null, uuid: "f-1" },
  { id: 2, name: "Staging", parent_id: null, color: null, uuid: "f-2" },
];

const server = (id, name, host, folder_id, extra = {}) => ({
  id, name, host, port: 22, username: "deploy", has_password: false, credential_id: null, folder_id,
  proxy_type: "none", proxy_host: "", proxy_port: 1080, tunnels: "[]", auth_type: "key", key_id: 1,
  autostart: false, mirrors: "[]", color: null, notes: "", run_on_connect: "", run_on_connect_every: false,
  jump_host_id: null, updated_at: null, edited_by: null, position: id, os: "ubuntu", terminal_theme: null,
  uuid: `s-${id}`, highlight: null, protocol: "ssh", held: [], ...extra,
});

// web1, web2 and db1 open by themselves (autostart), so their tabs are there.
const servers = [
  server(1, "web1", "192.0.2.11", 1, { autostart: true }),
  server(2, "web2", "192.0.2.12", 1, { autostart: true }),
  server(3, "db1", "192.0.2.21", 1, { autostart: true, os: "debian" }),
  server(4, "build", "203.0.113.5", null, { os: "rocky" }),
  server(5, "staging-api", "203.0.113.40", 2),
];
// For timing many terminals at once (perf.mjs): `mock-servers` in
// localStorage adds that many more servers, each opening by itself.
const extra = Number(localStorage.getItem("mock-servers") ?? 0);
for (let i = 0; i < Math.min(extra, 200); i++) {
  servers.push(server(100 + i, `node${String(i + 1).padStart(2, "0")}`, `198.51.100.${(i % 250) + 1}`, 2, { autostart: true }));
}
const byId = (id) => servers.find((s) => s.id === id);
const sessionServer = (sid) => byId(Number(String(sid).replace(/^session-/, "").split("::")[0]));

// ── The AI state: one AI tab sharing the three tabs, a request on all three
// waiting, a copy_files request, and a follow running.
const aiServer = (s, level = 1) => ({ session_id: `session-${s.id}`, name: s.name, server_id: s.id, allow_all: false, level, root_via: null });
const pending = (id, s, extra) => ({
  id, tab_id: "ai-1", tab_label: "Claude Code", session_id: `session-${s.id}`, server: s.name, user: "deploy",
  command: "", root: false, as_root: false, root_via: null, password: "none", timeout_secs: 60, asked_at: ago(0),
  server_id: s.id, ask_rule: null, can_allow_always: true, note: null, group: null, copy: null, follow: false, ...extra,
});
const groupCmd = "systemctl status nginx --no-pager";
const group = { id: 7, size: 3 };
const aiState = {
  enabled: true,
  audit_log: "ai-audit.log",
  tabs: [{ id: "ai-1", label: "Claude Code", servers: [aiServer(byId(1)), aiServer(byId(2)), aiServer(byId(3))], intruder_at: null }],
  // The AI asking for a server by name (ai-server-ask.png): with
  // `mock-server-ask` in localStorage.
  server_asks: localStorage.getItem("mock-server-ask")
    ? [{ id: 21, tab_id: "ai-1", tab_label: "Claude Code", asked: "staging-api", server_id: 5, server_name: "staging-api", many: false, asked_at: ago(0) }]
    : [],
  pending: [
    pending(11, byId(1), { command: groupCmd, group }),
    pending(12, byId(2), { command: groupCmd, group }),
    pending(13, byId(3), { command: groupCmd, group }),
    pending(14, byId(2), {
      command: "copy /var/log/nginx/error.log from web2 to /home/me/ai-downloads",
      can_allow_always: false,
      copy: { direction: "pull", source: "/var/log/nginx/error.log", destination: "/home/me/ai-downloads", local: null, replace: false, into: "/home/me/ai-downloads/web2" },
    }),
  ],
  running: [
    { id: 21, tab_label: "Claude Code", session_id: "session-3", server: "db1", command: "journalctl -fu postgresql", started_at: ago(4),
      ran_without_asking: null, as_root: false, root_via: null, follow: { bytes_seen: 48200 } },
  ],
  history: [
    { id: 31, tab_label: "Claude Code", server: "web1", command: "df -h /var", root: false, as_root: false, root_via: null, outcome: "done",
      exit_code: 0, output_bytes: 412, hidden: 0, cut: false, timed_out: false, error: null, at: ago(6), ran_without_asking: null, follow: null },
    { id: 32, tab_label: "Claude Code", server: "web2", command: "uptime", root: false, as_root: false, root_via: null, outcome: "done",
      exit_code: 0, output_bytes: 70, hidden: 0, cut: false, timed_out: false, error: null, at: ago(8), ran_without_asking: "on your allow list", follow: null },
  ],
};

// ── Approval levels: "Ask every time" for every server, web1 at "Run my
// allowed commands", and a few rules.
const trustRules = [
  { id: 1, scope: "default", scope_id: 0, name: "", kind: "allow", pattern: "uptime" },
  { id: 2, scope: "default", scope_id: 0, name: "", kind: "allow", pattern: "df -h" },
  { id: 3, scope: "default", scope_id: 0, name: "", kind: "ask", pattern: "reboot" },
  { id: 4, scope: "server", scope_id: 1, name: "web1", kind: "allow", pattern: "systemctl status *" },
];
const trustLevels = [{ scope: "server", scope_id: 1, name: "web1", level: 2 }];
const trustScope = ({ scope, id }) => ({
  own_level: scope === "default" ? 1 : trustLevels.find((l) => l.scope === scope && l.scope_id === id)?.level ?? null,
  inherited_level: 1,
  inherited_from: "",
  rules: trustRules.filter((r) => r.scope === scope && (scope === "default" || r.scope_id === id)),
  inherited_allow: scope === "default" ? 0 : 2,
  inherited_ask: scope === "default" ? 0 : 1,
});

const aiSettings = {
  enabled: true, flash: true, available: true,
  tools: [
    { id: "claude", label: "Claude Code", path: "/home/me/.local/bin/claude" },
    { id: "codex", label: "Codex", path: null },
  ],
  pull_folders: ["/home/me/ai-downloads", "/home/me/projects/incident-notes"],
};

// ── Remote folders, for the bulk push's check: what's already there.
// db1 has no ~/releases, so it's left out.
const remoteListing = (sid, path) => {
  const s = sessionServer(sid);
  if (s?.name === "db1" && /releases/.test(path)) throw "No such file or directory";
  const home = "/home/deploy";
  const dir = !path || path === "~" || path === "." ? home : path.replace(/^~/, home);
  const there = { web1: ["app.conf", "release-notes.txt"], web2: ["app.conf"] }[s?.name] ?? [];
  return { current_path: dir, entries: there.map((name) => ({ name, is_dir: false, size: 1200, modified: ago(600), permissions: "rw-r--r--" })) };
};

// What each terminal shows: a login, a prompt and a command.
const prompt = (name) => `\x1b[1;32mdeploy@${name}\x1b[0m:\x1b[1;34m~\x1b[0m$ `;
// web2's shows the end of a log, for the highlighting picture.
const logText = (name) => [
  `${prompt(name)}tail -n 8 /var/log/app.log\r\n`,
  "2026-11-07 10:41:02 INFO    api   Connected to 192.0.2.21:5432 (database primary)\r\n",
  "2026-11-07 10:41:05 WARNING api   Slow query took 2350 ms, see https://wiki.example.com/slow-queries\r\n",
  "2026-11-07 10:41:09 ERROR   api   Connection refused by 192.0.2.30:6379, retrying in 5s\r\n",
  "2026-11-07 10:41:14 OK      cache reconnected\r\n",
  "2026-11-07 10:41:20 INFO    net   eth0 is up, MAC 52:54:00:12:34:56, fe80::5054:ff:fe12:3456\r\n",
  "2026-11-07 10:41:31 FAILED  api   Health check /api/status returned 503\r\n",
  "2026-11-07 10:41:40 SUCCESS ci    Deploy 2.4.1 finished\r\n",
  "2026-11-07 10:41:52 CRITICAL db1  Disk /var is 97% full\r\n",
  prompt(name),
].join("");
const terminalText = (name) => name === "web2" ? logText(name) : [
  "Welcome to Ubuntu 24.04.1 LTS (GNU/Linux 6.8.0-45-generic x86_64)\r\n\r\n",
  "Last login: Sat Nov  7 09:12:44 2026 from 198.51.100.7\r\n",
  `${prompt(name)}uptime\r\n 10:42:17 up 23 days,  4:11,  1 user,  load average: 0.21, 0.18, 0.12\r\n`,
  prompt(name),
].join("");

const later = (fn, ms = 60) => setTimeout(fn, ms);
// Each terminal's output channel, for the echo of what's typed.
const outputs = new Map();

// Commands the page asked for that have no answer here (take.mjs prints them).
window.__unhandled = new Set();

mockIPC((cmd, args = {}) => {
  switch (cmd) {
    // The profile page: one profile, opened on this Windows account.
    case "list_profiles": return ["demo"];
    case "quick_unlock_support": return { method: "windows-auto", ready: true };
    case "quick_unlock_status": return { method: "windows-auto", can_open: true };
    case "quick_unlock": return null;
    case "get_cloud_enabled": return false;
    case "cloud_status": return { signed_in: false, email: null };
    case "check_for_updates":
      return { current: "0.4.0", latest: null, has_update: false, release_url: null, main_ahead: null, main_url: null, upstream_ahead: null, upstream_url: null };
    case "app_info": return { version: "0.4.0", updates: false, github_repo_url: "https://github.com/paulm981/Submarine", commit: null, build: null };

    // The profile's contents.
    case "get_servers": return servers;
    case "get_folders": return folders;
    case "get_credentials": return [];
    case "get_ssh_keys": return [];
    case "get_commands": return [];
    case "get_notes": return [];
    case "highlight_rules_list": return [];
    case "list_system_fonts": return [];
    case "list_local_shells":
      return {
        shells: [
          { id: "login", label: "bash", group: "", detail: "your login shell" },
          { id: "zsh", label: "zsh", group: "", detail: "/usr/bin/zsh" },
          { id: "sh", label: "sh", group: "", detail: "/bin/sh" },
          { id: "tmux", label: "tmux", group: "", detail: "/usr/bin/tmux" },
        ],
        notes: [], default_id: "login", custom: null, custom_missing: false,
        ai_tools: [{ id: "claude", label: "Claude Code", group: "AI tools", detail: null }, { id: "codex", label: "Codex", group: "AI tools", detail: null }],
      };

    // The profile syncs through a git repository of the user's own.
    case "store_status": return { kind: "git", location: "git@git.example.com:team/submarine-profiles.git", branch: "submarine-sync" };
    case "sync_now": return { pushed: 0, pulled: 0 };
    case "profile_share_status":
      return { profile: "demo", share_id: null, role: null, identity_unlocked: false, signed_in: false, email: null, cloud_server: null, current_server: "", custom_server: false, cloud_deleted: false };
    case "profile_sync_stats":
      return {
        total_records: 42, vault_bytes: 81920, recent_edits: [], unreadable: 0,
        diff: { in_sync: 42, needs_push: 0, needs_pull: 0, out_of_sync_nodes: [] },
        devices: [{ name: "laptop", written: now - 4 * 3600_000, this_one: true }, { name: "desktop", written: now - 3 * 86400_000, this_one: false }],
      };

    // A server tab connects at once, and its terminal shows a prompt.
    case "initiate_connection":
      later(() => emit(`connection-success-${args.sessionId}`, {}));
      return null;
    case "open_terminal": {
      outputs.set(args.terminalId, args.onOutput);
      const name = sessionServer(args.sessionId)?.name ?? "server";
      later(() => args.onOutput?.onmessage?.(new TextEncoder().encode(terminalText(name)).buffer), 150);
      return null;
    }
    case "sftp_list_dir": return remoteListing(args.sessionId, args.path);
    // A shell on this computer, in Home.
    case "local_terminal_open":
      later(() => args.onOutput?.onmessage?.(new TextEncoder().encode("\x1b[1;32mme@laptop\x1b[0m:\x1b[1;34m~\x1b[0m$ ").buffer), 150);
      return "bash";
    // A shell echoes what's typed, as a real one does.
    case "write_terminal_data": {
      window.__writes = (window.__writes ?? 0) + 1;
      const out = outputs.get(args.terminalId);
      later(() => out?.onmessage?.(new Uint8Array(args.data).buffer), 5);
      return null;
    }

    // Kept sessions: web1 has two in tmux, one of them open in its tab.
    case "keep_session_state": return { kept: true, running: false };
    case "keep_session_list": return {
      tool: "tmux",
      sessions: [
        { name: "sub-s-1-1", pid: 48211, created: Math.floor(now / 1000) - 3 * 86400, attached: true, ours: true, terminal: 1 },
        { name: "sub-s-1-2", pid: 51907, created: Math.floor(now / 1000) - 5 * 3600, attached: false, ours: true, terminal: 2 },
      ],
    };

    // Nothing to show in these, but the shapes they expect.
    case "monitor_list": return [];
    case "monitor_get_settings": return { interval_secs: 30, connect_timeout_secs: 5, poll_timeout_secs: 10, outage_threshold: 3, notify_on_outage: false, beep_on_outage: false, beep_cooldown_secs: 60 };
    case "held_deletions_list": return [];
    case "get_jump_login_limit": return 4;
    case "share_key_state": return { current: null, pending: null, grant_unread: false, grant_refused: false, identity_unlocked: false };

    // AI tools.
    case "ai_state": return aiState;
    case "ai_settings": return aiSettings;
    case "ai_trust_view": return { default_level: 1, levels: trustLevels, rules: trustRules };
    case "ai_trust_scope": return trustScope(args);

    // The bulk push: three files picked.
    case "bulk_pick_files": return [
      { path: "/home/me/release/app.conf", name: "app.conf", size: 2310 },
      { path: "/home/me/release/app-2.4.1.tar.gz", name: "app-2.4.1.tar.gz", size: 18_400_000 },
      { path: "/home/me/release/release-notes.txt", name: "release-notes.txt", size: 5120 },
    ];
  }
  window.__unhandled.add(cmd);
  return null;
}, { shouldMockEvents: true });

// The profile opened last, so it opens by itself; and terminals are kept.
try {
  localStorage.setItem("submarine-last-profile", "demo");
  localStorage.setItem("submarine-keep-session-default", "1");
  localStorage.setItem("submarine-keep-session-seen", "1");
} catch { /* the first profile is picked */ }

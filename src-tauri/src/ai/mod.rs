//! AI help on open sessions, without sharing keys (#32). An AI tool started
//! from Home's menu reaches Submarine over a local channel (channel.rs,
//! through `submarine --mcp`, helper.rs) and asks it to run commands on the
//! servers you've shared with that AI tab. Submarine shows each request in
//! the AI panel and runs it once you click Allow, or at once where your
//! approval level and rules say so (trust.rs, #115), on its own SSH channel
//! (exec.rs), and gives back the output with known secrets hidden
//! (filter.rs), marked as the server's words, not instructions (boundary.rs,
//! #178). Every request and decision goes in the audit log (audit.rs).
//!
//! The AI never gets a key or a password, and never connects to a server.
//! It can also ask to copy files between this computer and its servers
//! (`copy_files`, copy.rs, #159), on one card, with the app's own transfers,
//! and to follow a log as it grows (`follow`, follow.rs, #179): allowed
//! once, then read a little at a time.
//!
//! - **Off by default** (Settings › AI tools). Off, nothing listens, Home
//!   lists no AI tools, and turning it off ends every AI tab's access.
//! - **Each AI tab has its own token,** in its tool's environment only,
//!   and one connection at a time: another with its token is refused, and
//!   the panel warns. Servers are shared with one AI tab, checked on every
//!   request.
//! - **Not kept apart from a hostile program running as you,** which can
//!   read the token from the tool's environment, edit the audit log, or
//!   click for you. The helper checks it reaches the app's own process
//!   (channel.rs), so such a program can't stand in for the app; the rest
//!   is FORK.md's advice to run the AI tool in its sandbox.
//! - **The AI tool starts without your SSH agent** and the usual cloud
//!   logins (CREDENTIAL_ENV): SSH_AUTH_SOCK names a place with no agent,
//!   and the logins are taken out, so what it runs doesn't find them there.
//! - **Access ends** when the AI tab closes, when the server's tab closes or
//!   reconnects, when you stop sharing, or when Submarine quits; anything
//!   still waiting for approval is cancelled and anything running stopped,
//!   on the server too. "Allow everything here until this AI tab closes"
//!   (SharedServer::allow_all) ends with it: it's kept on the server's place
//!   in the AI tab's list, which every end of access takes away.
//! - **The AI can't change how much it may do.** Levels and rules are read
//!   here, as each request comes (`policy`), and changed only by the Tauri
//!   commands the app's own pages call. No MCP tool reaches them.
//! - **Root without the password (#120).** `run_command` with `as_root` runs
//!   the command through sudo or dzdo (exec::run_root, with the same code as
//!   SFTP root mode). Where the server wants a password, the card asks you
//!   for it; it goes only to the tool's own prompt, one attempt each time.
//!   What worked is kept in the server's place in the AI tab's list
//!   (SharedServer::root), so every end of access forgets it, and is never
//!   shared with the server tab's own root access, either way. It's never in
//!   the output, the AI's answer, the audit log, the app log or an error.

mod ask_server;
mod audit;
mod boundary;
mod channel;
mod claude;
mod codex;
mod copy;
mod exec;
mod filter;
mod follow;
#[cfg(test)]
mod fuzz_tests;
pub mod helper;
mod mcp;
mod read_only;
pub(crate) mod tools;
pub(crate) mod trust;

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch};
use zeroize::Zeroizing;

use mcp::ToolResult;

/// Kept with the Home terminal's settings (local_open::read_setting), on
/// this device only: AI settings never sync.
const SETTINGS_FILE: &str = "ai_tools.json";

/// The id Home's menu names Claude Code by.
pub(crate) const CLAUDE_ID: &str = "ai:claude";

/// What an AI tool starts without (#116): your SSH agent's process id, and
/// the usual cloud and code-host logins kept in the environment. With them,
/// a script it writes could reach your servers or accounts itself, with no
/// approval and no log. It keeps everything else it needs: PATH, HOME,
/// proxies, and its own login (claude.rs keeps a cloud's variables when
/// Claude Code logs in through that cloud).
///
/// SSH_AUTH_SOCK isn't taken out but set to where no agent is
/// (channel::no_agent_at, in start_tab): Windows OpenSSH's ssh looks for
/// its agent's pipe (`\\.\pipe\openssh-ssh-agent`) when the variable is
/// unset, not when it names somewhere else.
///
/// Not a wall. ssh run by the AI tool still uses an agent named by
/// IdentityAgent in ~/.ssh/config, an open ControlMaster connection, and
/// key files with no passphrase (or one in the macOS Keychain); and a
/// program it runs can look for an agent's socket or pipe itself, or read
/// key files, unless the tool's own sandbox stops it.
pub(crate) const CREDENTIAL_ENV: &[&str] = &[
    // Your SSH agent (SSH_AUTH_SOCK: see above).
    "SSH_AGENT_PID",
    // AWS.
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_SECURITY_TOKEN",
    "AWS_PROFILE",
    "AWS_DEFAULT_PROFILE",
    "AWS_BEARER_TOKEN_BEDROCK",
    // Azure (and Terraform's name for its secret).
    "AZURE_CLIENT_SECRET",
    "AZURE_CLIENT_ID",
    "AZURE_TENANT_ID",
    "AZURE_CLIENT_CERTIFICATE_PATH",
    "AZURE_CLIENT_CERTIFICATE_PASSWORD",
    "AZURE_USERNAME",
    "AZURE_PASSWORD",
    "ARM_CLIENT_SECRET",
    // Google Cloud.
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CREDENTIALS",
    "GOOGLE_OAUTH_ACCESS_TOKEN",
    "CLOUDSDK_AUTH_ACCESS_TOKEN",
    // GitHub and GitLab.
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GITLAB_TOKEN",
    // Other clouds and secret stores.
    "DIGITALOCEAN_TOKEN",
    "DIGITALOCEAN_ACCESS_TOKEN",
    "HCLOUD_TOKEN",
    "LINODE_TOKEN",
    "CLOUDFLARE_API_TOKEN",
    "OS_PASSWORD",
    "VAULT_TOKEN",
];

// The limits (#32 row 11).
const MAX_COMMAND: usize = 16 * 1024;
const DEFAULT_TIMEOUT: u64 = 60;
pub(crate) const MAX_TIMEOUT: u64 = 600;
/// What's read from the server, and shown to you, at most; the command is
/// stopped past it.
const OUTPUT_CAP: usize = 1024 * 1024;
/// What the AI gets at most: the start and the end of the filtered output.
const AI_HEAD: usize = 32 * 1024;
const AI_TAIL: usize = 32 * 1024;
/// Requests an AI tab may make in a minute, and have waiting at once.
const RATE_MAX: usize = 20;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const MAX_WAITING: usize = 3;
/// The most servers one `run_on_servers` request names (#158).
const MAX_GROUP: usize = 50;
/// What the AI gets back from one `run_on_servers` request, its servers'
/// parts together: each gets its share, cut in the middle.
const GROUP_AI_BUDGET: usize = 128 * 1024;
/// What a server sent gets this much of its part at least, whatever
/// Submarine's own words there take.
const MIN_SERVER_SHARE: usize = 512;
/// Commands one AI tab may have running at once on one server, those that
/// ran without asking included.
const MAX_RUNNING: usize = 4;
/// What the log keeps at most of a request's command (as written in its
/// JSON), and of a server name: the AI may send anything there, up to MCP's
/// 1 MB, and may ask, withdraw and ask again without you allowing anything.
const MAX_LOGGED_COMMAND: usize = 4 * 1024;
const MAX_LOGGED_NAME: usize = 256;
/// And of a note (why it was refused, an error), which may quote either.
const MAX_LOGGED_NOTE: usize = 2 * 1024;
/// Finished requests the panel keeps.
const HISTORY: usize = 20;
/// A note you give with Deny, at most, in characters.
const MAX_DENY_NOTE: usize = 500;
/// How long a second connection for an AI tab waits for the first to end
/// before it's refused: Claude Code reconnecting (`/mcp`) ends its old
/// helper and starts a new one, which may connect a moment before the app
/// has seen the old connection close.
const RECONNECT_GRACE: Duration = Duration::from_secs(5);
/// The panel's event.
const CHANGED: &str = "ai-changed";

#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    enabled: bool,
    /// Flash the taskbar when a request waits for you and Submarine isn't
    /// in front. On unless turned off.
    #[serde(default = "on")]
    flash: bool,
    /// The folders on this computer `copy_files` may pull into (#159):
    /// none until you add one. Only Settings changes them; no MCP tool
    /// reaches them.
    #[serde(default)]
    pull_folders: Vec<String>,
}

fn on() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: false, flash: true, pull_folders: Vec::new() }
    }
}

type ExecFuture = Pin<Box<dyn Future<Output = Result<exec::Exec, String>> + Send>>;
/// Runs a command on a session's connection: the real one over SshState, or
/// a test server's. `(session_id, command, timeout, stop)`; see exec::run.
type Executor = Arc<dyn Fn(String, String, Duration, watch::Receiver<bool>) -> ExecFuture + Send + Sync>;
type RootFuture = Pin<Box<dyn Future<Output = exec::RootRun> + Send>>;
/// Runs a command as root (#120) on a session's connection, as Executor
/// does: `(session_id, job, timeout, stop)`; see exec::run_root.
type RootExecutor = Arc<dyn Fn(String, RootJob, Duration, watch::Receiver<bool>) -> RootFuture + Send + Sync>;

/// A command to run as root, and how root is had.
pub(crate) struct RootJob {
    command: String,
    how: exec::RootHow,
    /// Told once the tool has taken the password (or needed none) and the
    /// command started.
    started: Option<oneshot::Sender<()>>,
}

/// What an AI tab has for root on a shared server (#120): the tool, a root
/// shell's words, and the password you typed for it there, if the server
/// asked for one (zeroised when the last copy goes). Never shown, logged or
/// sent.
#[derive(Clone)]
struct KeptRoot(Arc<crate::ssh_manager::RootAccess>);

impl std::fmt::Debug for KeptRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeptRoot({})", self.0.tool)
    }
}

/// What the AI hears when root can't be had: never to get it some other
/// way (#232).
pub(crate) const NO_ROOT_BY_HAND: &str = "Don't add sudo, dzdo or su to a command yourself.";

/// What a request's card asks of the password, for what the AI is told
/// when it's denied (#232).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Asked {
    /// Nothing: root didn't want a password, or it isn't as root.
    Nothing,
    /// The tool (dzdo or sudo, if known) wants the login's password.
    Password(Option<&'static str>),
    /// The tool turned down the password typed for it.
    Refused(Option<&'static str>),
}

impl Asked {
    /// What follows "Denied by the user." for the AI.
    fn for_ai(self, server: &str) -> String {
        let tool = |t: Option<&str>| t.map(str::to_string).unwrap_or_else(|| "sudo or dzdo".into());
        match self {
            Asked::Nothing => String::new(),
            Asked::Password(t) => format!(" Couldn't get root on {server}: {} needs the user's password, and they didn't give it. {NO_ROOT_BY_HAND}", tool(t)),
            Asked::Refused(t) => format!(" Couldn't get root on {server}: {} turned down the password the user typed. {NO_ROOT_BY_HAND}", tool(t)),
        }
    }
}

/// For a request as root, whether its card asks for the login's password
/// (for sudo or dzdo).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum PasswordAsk {
    /// No field: not as root, or this AI tab has what it needs there.
    None,
    /// Nothing is known yet: a field you may leave empty, for a server that
    /// doesn't ask.
    Optional,
    /// The server asked for one.
    Needed,
    /// The one typed was turned down.
    Wrong,
}
/// What holds for a saved server (its local id; None for one not saved):
/// read from the vault in the app, set by hand in tests. Read only.
type PolicyLookup = Arc<dyn Fn(Option<i64>) -> trust::Policy + Send + Sync>;

/// list_shared's answer (#176), compact JSON in this order.
#[derive(Serialize)]
struct Listed<'a> {
    servers: Vec<ListedServer<'a>>,
    /// The folders on this computer the user allowed for pulls.
    copy_into: Vec<String>,
    limits: ListedLimits,
}

#[derive(Serialize)]
struct ListedServer<'a> {
    name: &'a str,
    /// The OS it reported (trust::Policy's `os`), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    os: Option<String>,
    /// trust::Requests::phrase.
    requests: &'static str,
}

#[derive(Serialize)]
struct ListedLimits {
    files_per_copy: u64,
    bytes_per_copy: u64,
    servers_per_request: usize,
    timeout_seconds: ListedTimeout,
    /// What the AI gets of one command's output at most.
    output_bytes_to_you: usize,
    /// Follows (#179) an AI tab may have open at once, and what each keeps
    /// between reads.
    follows_per_tab: usize,
    follow_buffer_bytes: usize,
}

#[derive(Serialize)]
struct ListedTimeout {
    default: u64,
    max: u64,
}

/// A server shared with an AI tab.
#[derive(Serialize, Clone, Debug)]
struct SharedServer {
    session_id: String,
    /// What the AI calls it: the tab's name, made unique for this AI tab.
    name: String,
    /// Who its tab logged in as, which commands run as; None if not known.
    user: Option<String>,
    /// The saved server its tab was opened for (its local id), whose level
    /// and rules hold; None for one not saved.
    server_id: Option<i64>,
    /// "Allow everything here until this AI tab closes": as level 3, for
    /// this AI tab only. In memory only, and gone when this goes.
    allow_all: bool,
    /// Its level now, for the tab's marker: filled in by `snapshot`.
    level: u8,
    /// At "Run commands that only read", whether they run by themselves
    /// here (trust::Policy::reads_alone): filled in by `snapshot`.
    reads_alone: bool,
    /// What this AI tab has for root there (#120), once a request as root
    /// worked: in memory only, gone when this goes (every end of access).
    /// Never in a snapshot.
    #[serde(skip)]
    root: Option<KeptRoot>,
    /// The tool root goes through there, once known (sudo or dzdo): filled
    /// in by `snapshot`.
    root_via: Option<&'static str>,
}

struct Tab {
    id: String,
    label: String,
    token: Zeroizing<String>,
    servers: Vec<SharedServer>,
    /// When it last asked, for the rate limit.
    asked: VecDeque<Instant>,
    /// When "rate limited" was last logged for it: once a window, not once
    /// a call, so the AI can't fill the log by asking too often.
    rate_logged: Option<Instant>,
    /// Set when its access ends: its connections close.
    closed: watch::Sender<bool>,
    /// The number of its one live connection, 0 for none.
    live: watch::Sender<u64>,
    /// When another connection with its token was refused, for the panel.
    intruder_at: Option<String>,
    /// Its follows (#179), running or ended and not read to the end yet.
    follows: Vec<follow::Entry>,
}

#[derive(Serialize, Clone)]
struct PendingView {
    id: u64,
    tab_id: String,
    tab_label: String,
    session_id: String,
    server: String,
    /// Who it runs as: the server tab's login user, if known.
    user: Option<String>,
    command: String,
    /// It runs as root: the login user is root, the command asks for root
    /// (asks_for_root), or the AI asked to run it as root.
    root: bool,
    /// The AI asked to run it as root (`as_root`, #120).
    as_root: bool,
    /// What it goes through as root, once known: sudo or dzdo.
    root_via: Option<&'static str>,
    /// For a request as root, whether the card asks for the password.
    password: PasswordAsk,
    timeout_secs: u64,
    asked_at: String,
    /// The saved server it's for, whose lists "Always allow" adds to.
    server_id: Option<i64>,
    /// The always-ask rule that made it wait, if one did.
    ask_rule: Option<String>,
    /// "Always allow this command here" is offered: it's one simple command
    /// (trust::is_simple), on a saved server, that doesn't ask for root and
    /// no always-ask rule caught.
    can_allow_always: bool,
    /// Why it asks when the server's settings would otherwise not: they
    /// couldn't be read, or a change to them waits for your OK.
    note: Option<String>,
    /// One of a `run_on_servers` request's servers (#158): the panel shows
    /// them as one card, answered together.
    group: Option<GroupView>,
    /// For a `copy_files` request (#159): what it copies, where. Its
    /// `command` is then a one-line summary of it, for the lists and the log.
    copy: Option<copy::CopyView>,
    /// A `follow` request (#179): it keeps running until it's stopped, at
    /// most `timeout_secs`.
    follow: bool,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
struct GroupView {
    id: u64,
    /// How many servers the request named.
    size: usize,
}

/// What a request is (#179): run once, or followed until it's stopped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Once,
    Follow,
}

/// A request that passed its checks (AiState::admit), waiting for you or,
/// allowed by your settings, in the running list: what the rest needs.
struct Admitted {
    req: Req,
    /// Your answer, while it waits for you.
    rx: Option<oneshot::Receiver<Answer>>,
    /// How it was allowed at once, by your settings.
    auto: Option<String>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    /// Its card, as it's shown now or if it comes back to ask for the root
    /// password.
    card: PendingView,
}

/// Who asked for what, where: for the log and the panel's history.
struct Req {
    tab_id: String,
    session_id: String,
    label: String,
    server: String,
    user: Option<String>,
    command: String,
    id: u64,
    /// The history it was asked in (Shared::epoch).
    epoch: u64,
    /// The tool, as the log names it.
    request: &'static str,
    root: bool,
    as_root: bool,
}

impl Req {
    /// Its line in the audit log, for `event`.
    fn entry(&self, event: &'static str) -> audit::Entry {
        let mut e = audit::Entry::new(event, &self.label);
        e.id = Some(self.id);
        e.request = Some(self.request);
        e.server = Some(self.server.clone());
        e.user = self.user.clone();
        e.command = Some(for_log(&self.command, MAX_LOGGED_COMMAND));
        e.root = self.root;
        e.as_root = self.as_root;
        e
    }

    /// Its row in the panel's history, ended as `outcome`.
    fn done(&self, outcome: &'static str) -> Done {
        Done {
            view: DoneView {
                id: self.id,
                tab_label: self.label.clone(),
                server: self.server.clone(),
                command: self.command.clone(),
                root: self.root,
                as_root: self.as_root,
                root_via: None,
                outcome,
                exit_code: None,
                output_bytes: 0,
                hidden: 0,
                cut: false,
                timed_out: false,
                error: None,
                at: now(),
                ran_without_asking: None,
                follow: None,
            },
            output: Zeroizing::default(),
            ai_output: Zeroizing::default(),
        }
    }
}

/// Commands of AI tab `tab_id` running on `session_id`, for MAX_RUNNING:
/// follows (#179) have a limit of their own.
fn running_on(s: &Shared, tab_id: &str, session_id: &str) -> usize {
    s.running.iter().filter(|r| r.tab_id == tab_id && r.session_id == session_id && r.view.follow.is_none()).count()
}

/// What `early_decision` found.
struct Early {
    looked_up: Option<(Option<i64>, bool)>,
    root_cmd: bool,
    opens_shell: bool,
    policy: Option<trust::Policy>,
    decision: trust::Decision,
}

/// The requests of AI tab `tab_id` waiting for you: a `run_on_servers`
/// request counts once, however many servers it names, and a request for a
/// server (ask_server.rs) counts too.
fn waiting_requests(s: &Shared, tab_id: &str) -> usize {
    let mine = s.pending.iter().filter(|p| p.view.tab_id == tab_id);
    let groups: std::collections::BTreeSet<u64> = mine.clone().filter_map(|p| p.view.group.map(|g| g.id)).collect();
    let servers = s.server_asks.iter().filter(|a| a.view.tab_id == tab_id).count();
    mine.filter(|p| p.view.group.is_none()).count() + groups.len() + servers
}

/// One answer for the AI from a `run_on_servers` (or `copy_files`)
/// request's servers' parts, in the order it named them: each part cut to
/// its share of GROUP_AI_BUDGET, keeping its start and end. What a server
/// sent is cut first, inside its part, then marked as the server's
/// (boundary.rs), so no cut goes through a tag. An error only if every part
/// is one.
fn group_answer(parts: &[(String, ToolResult)]) -> ToolResult {
    let share = (GROUP_AI_BUDGET / parts.len().max(1)).max(2 * 1024);
    let failed = parts.iter().filter(|(_, r)| r.is_error).count();
    let n = parts.len();
    let mut text = format!("{n} server{}: {} done, {failed} not.\n", if n == 1 { "" } else { "s" }, n - failed);
    for (name, r) in parts {
        let ours = cut_middle(&r.text, share);
        let server_text = r.server_text.as_ref().map(|said| {
            let room = share.saturating_sub(ours.len() + boundary::extra(&said.server)).max(MIN_SERVER_SHARE);
            boundary::ServerText { server: said.server.clone(), text: cut_middle(&said.text, room) }
        });
        let body = ToolResult { text: ours, is_error: r.is_error, server_text }.for_ai();
        text.push_str(&format!("\n=== {name}{} ===\n{}\n", if r.is_error { " (not run, or failed)" } else { "" }, body.text));
    }
    ToolResult { text, is_error: failed == parts.len(), server_text: None }
}

/// `text` within `max` bytes: its start and end, and a line saying how
/// much was left out between them.
fn cut_middle(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let half = max / 2;
    let mut head = half;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - half;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}\n[… {} bytes left out here by Submarine …]\n{}", &text[..head], tail - head, &text[tail..])
}

/// What a `run_on_servers` request tells each of its servers' requests.
#[derive(Clone, Copy)]
struct GroupRef {
    id: u64,
    size: usize,
    /// Some server would ask, so they all do.
    ask: bool,
}

/// Your answer to a request.
enum Answer {
    /// Allowed; how (Decision::approved_by, or "you"), and for a request as
    /// root, the password you typed on its card, if you did.
    Allow { by: String, password: Option<Zeroizing<String>> },
    /// Denied, with your note, if you gave one.
    Deny(Option<String>),
}

const BY_YOU: &str = "you";

struct Pending {
    view: PendingView,
    decide: oneshot::Sender<Answer>,
    /// Its stop signal, moved to `Running` when you allow it.
    stop: watch::Sender<bool>,
}

#[derive(Serialize, Clone)]
struct RunningView {
    id: u64,
    tab_label: String,
    session_id: String,
    server: String,
    command: String,
    started_at: String,
    /// Why it runs without your Allow, if it does.
    ran_without_asking: Option<String>,
    /// It runs as root at the AI's asking (#120), and through what, once
    /// known.
    as_root: bool,
    root_via: Option<&'static str>,
    /// A follow (#179): what it's seen so far. Filled in by `snapshot`.
    follow: Option<follow::FollowView>,
}

struct Running {
    view: RunningView,
    tab_id: String,
    session_id: String,
    /// Stops it, on the server too (exec::run).
    stop: watch::Sender<bool>,
    /// A follow's output, once it's started (#179).
    feed: Option<Arc<follow::Feed>>,
}

/// A finished request's row in the panel. Its outputs are kept beside it
/// (`Done`) and fetched one at a time (`ai_output`), when you open the row:
/// not sent, or copied, with every change.
#[derive(Serialize, Clone)]
struct DoneView {
    id: u64,
    tab_label: String,
    server: String,
    command: String,
    root: bool,
    /// It ran as root at the AI's asking (#120), and through what.
    as_root: bool,
    root_via: Option<&'static str>,
    /// denied, cancelled, done, failed, stopped.
    outcome: &'static str,
    exit_code: Option<i64>,
    output_bytes: usize,
    hidden: usize,
    cut: bool,
    timed_out: bool,
    error: Option<String>,
    at: String,
    /// It ran without asking you, and why: `rule: <pattern>`, `level: run
    /// anything` or `session: allow everything`.
    ran_without_asking: Option<String>,
    /// A follow (#179): how it ended and what it saw. Its output isn't
    /// kept: it went to the AI a read at a time.
    follow: Option<follow::FollowDone>,
}

/// A finished request: its row, everything you see, and what the AI was
/// given.
struct Done {
    view: DoneView,
    /// The whole output, up to OUTPUT_CAP. Wiped when it's dropped.
    output: Zeroizing<String>,
    /// What the AI got.
    ai_output: Zeroizing<String>,
}

#[derive(Serialize, Clone)]
struct TabView {
    id: String,
    label: String,
    servers: Vec<SharedServer>,
    /// When another program tried to connect as this AI tab, if one did.
    intruder_at: Option<String>,
}

/// The AI panel's view of everything.
#[derive(Serialize, Clone)]
pub(crate) struct Snapshot {
    enabled: bool,
    tabs: Vec<TabView>,
    pending: Vec<PendingView>,
    /// The AI's requests for a server by name, waiting for you (ask_server).
    server_asks: Vec<ask_server::AskView>,
    running: Vec<RunningView>,
    history: Vec<DoneView>,
    audit_log: Option<String>,
}

#[derive(Default)]
struct Shared {
    enabled: bool,
    tabs: Vec<Tab>,
    pending: Vec<Pending>,
    /// Requests for a server by name, waiting for you (ask_server).
    server_asks: Vec<ask_server::Ask>,
    running: Vec<Running>,
    history: VecDeque<Done>,
    next_id: u64,
    /// Numbers the `run_on_servers` requests (GroupView).
    next_group: u64,
    /// Numbers the live connections (Tab::live).
    next_conn: u64,
    /// Counts the times the history was cleared: a request remembers its
    /// row only in the history it was asked in, so one that ends after the
    /// profile locked doesn't bring the old profile's output back.
    epoch: u64,
}

struct Inner {
    shared: Mutex<Shared>,
    listener: Mutex<Option<channel::Listener>>,
    exec: Executor,
    root_exec: RootExecutor,
    changed: Box<dyn Fn() + Send + Sync>,
    audit: audit::AuditLog,
    policy: PolicyLookup,
    /// Draws your eye to Submarine when a request waits for you (the
    /// taskbar flashing), if `flash` is on.
    attention: Box<dyn Fn() + Send + Sync>,
    flash: AtomicBool,
    /// Where `copy_files` may pull into (Settings::pull_folders).
    pull_folders: Mutex<Vec<PathBuf>>,
    /// Does a `copy_files` request's copying on one server: the app's own
    /// SFTP transfers, or a test's. None: copying isn't available.
    copier: Mutex<Option<copy::Copier>>,
    /// Runs a follow (#179): the app's SSH connections, or a test's. None:
    /// following isn't available.
    follower: Mutex<Option<follow::Follower>>,
    /// Finds a saved server by name for `request_server`: the vault, or a
    /// test's list. None: no server is found.
    server_lookup: Mutex<Option<ask_server::Lookup>>,
}

#[derive(Clone)]
pub struct AiState(Arc<Inner>);

/// What a connection gets as it asks for its AI tab (AiState::claim).
enum Claim {
    Got(Live),
    /// Another connection has it, and kept it past RECONNECT_GRACE.
    Taken,
    /// The tab's access has ended.
    Gone,
}

/// An AI tab's one live connection: dropped, the tab may take another.
struct Live {
    state: AiState,
    tab_id: String,
    conn: u64,
}

impl Drop for Live {
    fn drop(&mut self) {
        let s = self.state.lock();
        if let Some(tab) = s.tabs.iter().find(|t| t.id == self.tab_id) {
            // Only its own: the tab may be another by the same id by now.
            tab.live.send_if_modified(|c| {
                let ours = *c == self.conn;
                if ours {
                    *c = 0;
                }
                ours
            });
        }
    }
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

/// A command that asks for root: one of the tools that give it, as a word
/// of its own (`sudo x`, `/usr/bin/sudo`, `(su -)`), anywhere in it. Where
/// sudo or dzdo needs no password, that's root with no question asked, and
/// Submarine can't always tell.
///
/// A warning for you on the request's card, and at "Run my allowed
/// commands" a reason to ask (trust::decide), which only ever asks more. Not
/// a wall: it's still easy to dodge (`$(echo sudo)`, a copy of sudo under
/// another name, a script that calls sudo), so nothing may be allowed because it says no. Anything
/// that has to keep root out (step 3 of #32: running as another account)
/// must decide on its own `as_root` and treat "can't tell" as root.
#[cfg(test)]
pub(crate) fn asks_for_root(command: &str) -> bool {
    trust::asks_for_root(command)
}

/// Equal without saying where they differ.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The start and the end of `text` when it's longer than the AI is given.
fn shorten_for_ai(text: &str) -> String {
    if text.len() <= AI_HEAD + AI_TAIL {
        return text.to_string();
    }
    let mut head = AI_HEAD;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - AI_TAIL;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}\n… [{} bytes left out by Submarine] …\n{}", &text[..head], tail - head, &text[tail..])
}

/// Resolves once the AI withdraws its request (MCP's cancel, or its
/// connection closing), or once nothing can any more.
async fn withdrawal(cancel: &mut watch::Receiver<bool>) {
    let _ = cancel.wait_for(|c| *c).await;
}

/// `text` for the audit log: cut (at a character's edge) to what takes at
/// most `max` bytes in the log's JSON, escapes counted, so a line stays
/// small whatever the AI sends. Cut, it says how much was left out and ends
/// with the first 16 hex digits of the SHA-256 of all of it, to tell
/// commands apart.
fn for_log(text: &str, max: usize) -> String {
    // What serde_json writes for it.
    let escaped = |c: char| match c {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    };
    let mut used = 0;
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        used += escaped(c);
        if used > max {
            end = i;
            break;
        }
    }
    if end == text.len() {
        return text.to_string();
    }
    use sha2::Digest;
    let hash = hex::encode(sha2::Sha256::digest(text.as_bytes()));
    format!("{}… [{} bytes more; sha256 {}]", &text[..end], text.len() - end, &hash[..16])
}

/// A name for `wanted` that no other server of this AI tab has.
fn unique_name(servers: &[SharedServer], wanted: &str) -> String {
    let wanted = wanted.trim();
    let base = if wanted.is_empty() { "server" } else { wanted };
    if !servers.iter().any(|s| s.name == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|n| !servers.iter().any(|s| &s.name == n)).unwrap()
}

impl AiState {
    /// Every server asks every time, nothing flashes, and nothing runs as
    /// root.
    #[cfg(test)]
    fn new(exec: Executor, changed: Box<dyn Fn() + Send + Sync>, audit_dir: Option<PathBuf>) -> AiState {
        AiState::with(exec, no_root(), changed, audit_dir, Arc::new(|_| trust::Policy::default()), Box::new(|| {}))
    }

    fn with(
        exec: Executor,
        root_exec: RootExecutor,
        changed: Box<dyn Fn() + Send + Sync>,
        audit_dir: Option<PathBuf>,
        policy: PolicyLookup,
        attention: Box<dyn Fn() + Send + Sync>,
    ) -> AiState {
        AiState(Arc::new(Inner {
            shared: Mutex::new(Shared::default()),
            listener: Mutex::new(None),
            exec,
            root_exec,
            changed,
            audit: audit::AuditLog::new(audit_dir),
            policy,
            attention,
            flash: AtomicBool::new(true),
            pull_folders: Mutex::new(Vec::new()),
            copier: Mutex::new(None),
            follower: Mutex::new(None),
            server_lookup: Mutex::new(None),
        }))
    }

    /// The app's: commands run over SshState's connections, and the page
    /// hears of each change.
    pub(crate) fn for_app(app: &tauri::AppHandle) -> AiState {
        use tauri::{Emitter, Manager};
        let exec_app = app.clone();
        let exec: Executor = Arc::new(move |session_id, command, limit, stop| {
            let app = exec_app.clone();
            Box::pin(async move {
                let state = app.state::<crate::ssh_manager::SshState>();
                let handle = state.connections.lock().await.get(&session_id).cloned();
                let handle = handle.ok_or("That server's tab isn't connected right now.")?;
                exec::run(&handle, &command, limit, OUTPUT_CAP, stop).await
            })
        });
        // As root: on the same connection, with only what the AI tab has
        // for root there (RootJob), never the tab's own root access.
        let root_app = app.clone();
        let root_exec: RootExecutor = Arc::new(move |session_id, job: RootJob, limit, stop| {
            let app = root_app.clone();
            Box::pin(async move {
                let state = app.state::<crate::ssh_manager::SshState>();
                let handle = state.connections.lock().await.get(&session_id).cloned();
                let Some(handle) = handle else {
                    let why = "That server's tab isn't connected right now.".to_string();
                    return exec::RootRun::Failed { you: why.clone(), ai: format!("Couldn't run it: {why}") };
                };
                exec::run_root(&handle, job.how, &job.command, limit, OUTPUT_CAP, stop, job.started).await
            })
        });
        let emit_app = app.clone();
        let changed = Box::new(move || {
            let _ = emit_app.emit(CHANGED, ());
        });
        // The open profile's levels and rules, read as each request comes.
        // No profile open (or the vault busy failing): ask every time.
        let db_app = app.clone();
        let policy: PolicyLookup = Arc::new(move |server_id| {
            let db = db_app.state::<crate::DbState>();
            let guard = db.conn.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|conn| trust::policy_for(conn, server_id)).unwrap_or_else(trust::Policy::unreadable)
        });
        let attention_app = app.clone();
        let attention = Box::new(move || {
            #[cfg(not(target_os = "android"))]
            {
                let windows = attention_app.webview_windows();
                let window = windows.get("main").or_else(|| windows.values().next());
                if let Some(w) = window.filter(|w| !w.is_focused().unwrap_or(false)) {
                    // The taskbar button flashes until Submarine is in front
                    // (Windows), or is marked urgent (Linux).
                    let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
                }
            }
            #[cfg(target_os = "android")]
            let _ = &attention_app;
        });
        let state = AiState::with(exec, root_exec, changed, app.path().app_log_dir().ok(), policy, attention);
        let settings: Settings = crate::local_open::read_setting(app, SETTINGS_FILE).unwrap_or_default();
        state.0.flash.store(settings.flash, Ordering::Relaxed);
        state.set_server_lookup(ask_server::app_lookup(app.clone()));
        state.set_pull_folders(copy::kept_folders(&settings.pull_folders));
        state.set_copier(copy::app_copier(app.clone()));
        state.set_follower(follow::app_follower(app.clone()));
        if settings.enabled {
            let s = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = s.turn_on() {
                    crate::applog::write_line("ai", &format!("AI tools couldn't start: {e}"));
                }
            });
        }
        state
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn changed(&self) {
        (self.0.changed)();
    }

    /// Every line goes through here: whatever a caller put in them, the
    /// server's name and the note are cut to their limits.
    fn audit(&self, mut entry: audit::Entry) {
        for (field, max) in [(&mut entry.server, MAX_LOGGED_NAME), (&mut entry.note, MAX_LOGGED_NOTE)] {
            if let Some(text) = field.as_mut() {
                if text.len() > max {
                    *text = for_log(text, max);
                }
            }
        }
        self.0.audit.write(&entry);
    }

    /// Start listening. In a tokio runtime.
    fn turn_on(&self) -> Result<(), String> {
        let mut listener = self.0.listener.lock().unwrap_or_else(|e| e.into_inner());
        if listener.is_none() {
            let state = self.clone();
            let on: channel::OnConnect = Arc::new(move |stream| {
                let state = state.clone();
                tokio::spawn(async move { state.serve(stream).await });
            });
            *listener = Some(channel::listen(on).map_err(|e| format!("Couldn't open the local channel: {e}"))?);
        }
        drop(listener);
        self.lock().enabled = true;
        self.changed();
        Ok(())
    }

    /// Stop listening, and end every AI tab's access. Returns once nothing
    /// listens any more.
    async fn turn_off(&self) {
        self.lock().enabled = false;
        self.end_all_tabs();
        let listener = self.0.listener.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(l) = listener {
            l.close().await;
        }
        self.changed();
    }

    pub(crate) fn enabled(&self) -> bool {
        self.lock().enabled
    }

    /// The listener's address while AI tools are on.
    fn addr(&self) -> Option<String> {
        self.0.listener.lock().ok()?.as_ref().map(|l| l.addr().to_string())
    }

    // -- AI tabs ------------------------------------------------------------

    /// A new AI tab, `id` being its Home terminal's: its label, and the
    /// variables its tool starts with (the channel's address and its token).
    pub(crate) fn start_tab(&self, id: &str, short: &str) -> Result<(String, Vec<(String, String)>), String> {
        let addr = self.addr().ok_or("AI tools are off. Turn them on in Settings › AI tools.")?;
        let token = Zeroizing::new(channel::random_hex(32));
        let mut s = self.lock();
        if s.tabs.iter().any(|t| t.id == id) {
            return Err("That AI tab is already open.".into());
        }
        let label = (1..)
            .map(|n| if n == 1 { short.to_string() } else { format!("{short} {n}") })
            .find(|l| !s.tabs.iter().any(|t| &t.label == l))
            .unwrap();
        let no_agent = channel::no_agent_at(&addr);
        let env = vec![
            (channel::ENV_ADDR.to_string(), addr),
            (channel::ENV_TOKEN.to_string(), token.to_string()),
            (channel::ENV_PID.to_string(), std::process::id().to_string()),
            // ssh run by the AI tool doesn't find your agent through it
            // (CREDENTIAL_ENV).
            ("SSH_AUTH_SOCK".to_string(), no_agent),
        ];
        s.tabs.push(Tab {
            id: id.to_string(),
            label: label.clone(),
            token,
            servers: Vec::new(),
            asked: VecDeque::new(),
            rate_logged: None,
            closed: watch::channel(false).0,
            live: watch::channel(0).0,
            intruder_at: None,
            follows: Vec::new(),
        });
        drop(s);
        self.audit(audit::Entry::new("ai_tab_started", &label));
        self.changed();
        Ok((label, env))
    }

    /// The AI tab closed (or its tool ended): its token stops working, its
    /// connections close, what it asked for is cancelled. Nothing for a tab
    /// that isn't an AI tab.
    pub(crate) fn end_tab(&self, id: &str) {
        let tab = {
            let mut s = self.lock();
            let Some(at) = s.tabs.iter().position(|t| t.id == id) else { return };
            s.tabs.remove(at)
        };
        self.tab_ended(tab);
        self.changed();
    }

    /// What follows a tab taken off the list: its requests are cancelled
    /// first, so they're told access ended before its connections close
    /// (which withdraws them).
    fn tab_ended(&self, tab: Tab) {
        self.cancel_where(|tab_id, _| tab_id == tab.id);
        let _ = tab.closed.send(true);
        self.audit(audit::Entry::new("ai_tab_ended", &tab.label));
    }

    /// Every AI tab's access ends, as when each one closes: its token stops
    /// working, what it shared and asked for goes. Turning AI tools off does
    /// this, and so does locking the profile (close_profile), so nothing the
    /// profile shared outlives it. The channel itself stays as it is.
    ///
    /// The panel's history goes too, outputs wiped: it shows what ran on the
    /// profile's servers, which the next profile (or whoever is at the
    /// picker) mustn't see. In the same step as the tabs come off the list,
    /// so every request either was asked before (and the row it adds when
    /// this cancels it is kept out) or finds its tab gone and adds none.
    pub(crate) fn end_all_tabs(&self) {
        let (tabs, gone) = {
            let mut s = self.lock();
            s.epoch += 1;
            (std::mem::take(&mut s.tabs), std::mem::take(&mut s.history))
        };
        // Each output wiped as it's dropped.
        drop(gone);
        for tab in tabs {
            self.tab_ended(tab);
        }
        self.changed();
    }

    /// The tab whose token this is, and its "access ended" signal.
    fn tab_for_token(&self, token: &str) -> Option<(String, watch::Receiver<bool>)> {
        let s = self.lock();
        if !s.enabled {
            return None;
        }
        // Every token compared, so the time taken says nothing.
        let mut found = None;
        for t in &s.tabs {
            if same_token(&t.token, token) {
                found = Some((t.id.clone(), t.closed.subscribe()));
            }
        }
        found
    }

    /// Take the AI tab's one live connection: at once when it has none,
    /// else once the one it has ends, within RECONNECT_GRACE.
    async fn claim(&self, tab_id: &str) -> Claim {
        let deadline = tokio::time::Instant::now() + RECONNECT_GRACE;
        loop {
            let mut live = {
                let mut s = self.lock();
                s.next_conn += 1;
                let conn = s.next_conn;
                let Some(tab) = s.tabs.iter_mut().find(|t| t.id == tab_id) else { return Claim::Gone };
                if *tab.live.borrow() == 0 {
                    tab.live.send_replace(conn);
                    return Claim::Got(Live { state: self.clone(), tab_id: tab_id.to_string(), conn });
                }
                tab.live.subscribe()
            };
            let freed = tokio::time::timeout_at(deadline, live.wait_for(|c| *c == 0)).await.map(|r| r.is_ok());
            match freed {
                Ok(true) => continue,
                // Its access ended meanwhile.
                Ok(false) => return Claim::Gone,
                Err(_) => return Claim::Taken,
            }
        }
    }

    /// Another connection presented an AI tab's token while it had one: it
    /// was refused, and the panel says so on the tab.
    fn intruder(&self, tab_id: &str) {
        let label = {
            let mut s = self.lock();
            let Some(tab) = s.tabs.iter_mut().find(|t| t.id == tab_id) else { return };
            tab.intruder_at = Some(now());
            tab.label.clone()
        };
        let mut e = audit::Entry::new("connection_refused", &label);
        e.note = Some("another connection with this AI tab's token, while it had one".into());
        self.audit(e);
        crate::applog::write_line("ai", &format!("Refused a second connection for the AI tab {label}: only one is allowed."));
        self.changed();
    }

    /// The AI tab `id`'s "access ended" signal.
    #[cfg(test)]
    fn closed_of(&self, id: &str) -> watch::Receiver<bool> {
        self.lock().tabs.iter().find(|t| t.id == id).expect("the AI tab").closed.subscribe()
    }

    // -- Sharing -----------------------------------------------------------

    #[cfg(test)]
    fn share(&self, tab_id: &str, session_id: &str, name: &str) -> Result<(), String> {
        self.share_as(tab_id, session_id, name, None, None)
    }

    /// Share `session_id`, whose tab logged in as `user` and was opened for
    /// the saved server `server_id`, with `tab_id`.
    fn share_as(&self, tab_id: &str, session_id: &str, name: &str, user: Option<String>, server_id: Option<i64>) -> Result<(), String> {
        let mut s = self.lock();
        let tab = s.tabs.iter_mut().find(|t| t.id == tab_id).ok_or("That AI tab isn't open any more.")?;
        if tab.servers.iter().any(|x| x.session_id == session_id) {
            return Ok(());
        }
        let name = unique_name(&tab.servers, name);
        tab.servers.push(SharedServer {
            session_id: session_id.to_string(),
            name: name.clone(),
            user,
            server_id,
            allow_all: false,
            level: trust::ASK_EVERY_TIME,
            reads_alone: false,
            root: None,
            root_via: None,
        });
        let label = tab.label.clone();
        drop(s);
        let mut e = audit::Entry::new("shared", &label);
        e.server = Some(name);
        self.audit(e);
        self.changed();
        Ok(())
    }

    /// Stop sharing `session_id` with `tab_id`, or with every AI tab.
    fn unshare(&self, session_id: &str, tab_id: Option<&str>, why: &str) {
        let mut gone = Vec::new();
        {
            let mut s = self.lock();
            for t in s.tabs.iter_mut().filter(|t| tab_id.is_none_or(|id| id == t.id)) {
                if let Some(at) = t.servers.iter().position(|x| x.session_id == session_id) {
                    gone.push((t.id.clone(), t.label.clone(), t.servers.remove(at).name));
                }
            }
        }
        if gone.is_empty() {
            return;
        }
        for (id, label, name) in gone {
            self.cancel_where(|tab, session| tab == id && session == session_id);
            let mut e = audit::Entry::new("unshared", &label);
            e.server = Some(name);
            e.note = Some(why.to_string());
            self.audit(e);
        }
        self.changed();
    }

    /// The server's tab closed, or connects again: it's no longer shared.
    pub(crate) fn session_gone(&self, session_id: &str, why: &str) {
        self.unshare(session_id, None, why);
    }

    /// Stop: everything shared with this AI tab, and what it's asked for.
    fn stop_tab(&self, tab_id: &str) {
        let sessions: Vec<String> = self
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.servers.iter().map(|x| x.session_id.clone()).collect())
            .unwrap_or_default();
        for s in sessions {
            self.unshare(&s, Some(tab_id), "stopped");
        }
        self.cancel_where(|tab, _| tab == tab_id);
        self.changed();
    }

    /// Cancel the waiting requests, and stop the running ones, for which
    /// `which(tab_id, session_id)` holds.
    ///
    /// All under one lock with `decide`, so a request is always in one place
    /// (waiting, or running with its stop signal) and none slips between.
    fn cancel_where(&self, which: impl Fn(&str, &str) -> bool) {
        let mut s = self.lock();
        // Dropping a request's sender is what cancels it. A request for a
        // server has no server tab yet: it goes with its AI tab.
        s.pending.retain(|p| !which(&p.view.tab_id, &p.view.session_id));
        s.server_asks.retain(|a| !which(&a.view.tab_id, ""));
        s.running.retain(|r| {
            let stop = which(&r.tab_id, &r.session_id);
            if stop {
                let _ = r.stop.send(true);
            }
            !stop
        });
    }

    /// Your Allow or Deny, as a click on the card.
    #[cfg(test)]
    fn decide(&self, id: u64, allow: bool) -> Result<(), String> {
        self.answer(id, if allow { Answer::Allow { by: BY_YOU.into(), password: None } } else { Answer::Deny(None) })
    }

    /// Allow, with the root password typed on the card.
    #[cfg(test)]
    fn allow_with_password(&self, id: u64, password: &str) -> Result<(), String> {
        let password = typed_password(Some(password.to_string()))?;
        self.answer(id, Answer::Allow { by: BY_YOU.into(), password })
    }

    /// Your answer to a request. Allowed, it moves from waiting to running
    /// in the same step, with its stop signal, and only while its AI tab
    /// still has the server shared: so unsharing, Stop or a reconnect at the
    /// same moment either comes first (and it never runs) or finds it
    /// running (and stops it).
    fn answer(&self, id: u64, answer: Answer) -> Result<(), String> {
        let result = {
            let mut s = self.lock();
            let at = s.pending.iter().position(|p| p.view.id == id).ok_or("That request isn't waiting any more.")?;
            if matches!(answer, Answer::Allow { .. }) {
                let v = &s.pending[at].view;
                if running_on(&s, &v.tab_id, &v.session_id) >= MAX_RUNNING {
                    return Err(format!("{MAX_RUNNING} commands from this AI tab are already running on {}. Allow this one once one of them ends.", v.server));
                }
            }
            let p = s.pending.remove(at);
            let still_shared = s.tabs.iter().any(|t| t.id == p.view.tab_id && t.servers.iter().any(|x| x.session_id == p.view.session_id));
            match answer {
                Answer::Deny(note) => {
                    let _ = p.decide.send(Answer::Deny(note));
                    Ok(())
                }
                // Dropped: cancelled, as access has ended.
                Answer::Allow { .. } if !still_shared => Err("That server isn't shared with the AI any more.".to_string()),
                Answer::Allow { by, password } => {
                    // A password only ever goes with a request as root.
                    let password = password.filter(|_| p.view.as_root);
                    let v = &p.view;
                    s.running.push(Running {
                        view: RunningView {
                            id: v.id,
                            tab_label: v.tab_label.clone(),
                            session_id: v.session_id.clone(),
                            server: v.server.clone(),
                            command: v.command.clone(),
                            started_at: now(),
                            ran_without_asking: Some(by.clone()).filter(|b| b != BY_YOU),
                            as_root: v.as_root,
                            root_via: v.root_via,
                            follow: v.follow.then(follow::FollowView::default),
                        },
                        tab_id: v.tab_id.clone(),
                        session_id: v.session_id.clone(),
                        stop: p.stop.clone(),
                        feed: None,
                    });
                    if p.decide.send(Answer::Allow { by, password }).is_err() {
                        // Nobody is waiting for the answer any more.
                        s.running.pop();
                    }
                    Ok(())
                }
            }
        };
        self.changed();
        result
    }

    /// Deny, with a note the AI gets, so it can try another way. The note is
    /// only words for the AI: it can't allow anything.
    fn deny(&self, id: u64, note: Option<String>) -> Result<(), String> {
        let note = note
            .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|n| !n.is_empty())
            .map(|n| if n.chars().count() > MAX_DENY_NOTE { n.chars().take(MAX_DENY_NOTE).collect() } else { n });
        self.answer(id, Answer::Deny(note))
    }

    /// A waiting request's card, as it is now.
    fn pending_view(&self, id: u64) -> Result<PendingView, String> {
        self.lock().pending.iter().find(|p| p.view.id == id).map(|p| p.view.clone()).ok_or_else(|| "That request isn't waiting any more.".to_string())
    }

    /// "Allow everything here until this AI tab closes": this request is
    /// allowed, and from now on, as long as the server stays shared with
    /// this AI tab, its requests there run without asking, but for what the
    /// always-ask list catches. What else is waiting there now runs too,
    /// unless an always-ask rule made it wait.
    fn allow_session(&self, id: u64, password: Option<Zeroizing<String>>) -> Result<(), String> {
        let (others, label, server) = {
            let mut s = self.lock();
            let p = s.pending.iter().find(|p| p.view.id == id).ok_or("That request isn't waiting any more.")?;
            let (tab_id, session_id) = (p.view.tab_id.clone(), p.view.session_id.clone());
            let others: Vec<(u64, String, Option<i64>, bool, bool)> = s
                .pending
                .iter()
                .filter(|o| o.view.id != id && o.view.tab_id == tab_id && o.view.session_id == session_id && o.view.ask_rule.is_none())
                .map(|o| (o.view.id, o.view.command.clone(), o.view.server_id, o.view.as_root, o.view.copy.is_some()))
                .collect();
            let tab = s.tabs.iter_mut().find(|t| t.id == tab_id).ok_or("That AI tab isn't open any more.")?;
            let label = tab.label.clone();
            let shared = tab.servers.iter_mut().find(|x| x.session_id == session_id).ok_or("That server isn't shared with the AI any more.")?;
            shared.allow_all = true;
            (others, label, shared.name.clone())
        };
        let mut e = audit::Entry::new("allow_everything", &label);
        e.server = Some(server);
        e.by = Some(BY_YOU);
        e.note = Some("allow everything here until this AI tab closes".into());
        self.audit(e);
        let result = self.answer(id, Answer::Allow { by: BY_YOU.into(), password });
        // Each as it would be decided now (an always-ask rule added since
        // it was asked still holds it). One as root runs with what this AI
        // tab has for root there, or asks for the password again.
        for (other, command, server_id, as_root, is_copy) in others {
            let p = (self.0.policy)(server_id);
            // A copy's `command` is only its summary: it's decided as a copy.
            let d = if is_copy { copy::decision(&p, true) } else { trust::decide_parsed(&p, true, &trust::Parsed::new(&command), as_root) };
            if let Some(by) = d.approved_by() {
                let _ = self.answer(other, Answer::Allow { by, password: None });
            }
        }
        result
    }

    /// The requests of `run_on_servers` request `group` waiting for you.
    fn group_members(&self, group: u64) -> Vec<u64> {
        self.lock().pending.iter().filter(|p| p.view.group.is_some_and(|g| g.id == group)).map(|p| p.view.id).collect()
    }

    /// Your answer to a whole `run_on_servers` request (#158): each of its
    /// servers' requests answered as one would be. Some may not be allowed
    /// now (too many already running there, or unshared): they're named.
    fn decide_group(&self, group: u64, allow: bool, note: Option<String>) -> Result<(), String> {
        let members = self.group_members(group);
        if members.is_empty() {
            return Err("That request isn't waiting any more.".into());
        }
        let failed: Vec<String> = members
            .into_iter()
            .filter_map(|id| if allow { self.answer(id, Answer::Allow { by: BY_YOU.into(), password: None }) } else { self.deny(id, note.clone()) }.err())
            .collect();
        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed.join(" "))
        }
    }

    /// "Allow everything on these servers until this AI tab closes": each of
    /// a `run_on_servers` request's servers as "Allow everything here" on
    /// its own card.
    fn allow_session_group(&self, group: u64) -> Result<(), String> {
        let members = self.group_members(group);
        if members.is_empty() {
            return Err("That request isn't waiting any more.".into());
        }
        let failed: Vec<String> = members.into_iter().filter_map(|id| self.allow_session(id, None).err()).collect();
        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed.join(" "))
        }
    }

    /// A `run_on_servers` request counted against AI tab `tab_id`'s limits
    /// as one request: the rate, and how many may wait for you.
    fn admit_group(&self, tab_id: &str) -> Result<String, (String, String)> {
        let mut s = self.lock();
        let waiting = waiting_requests(&s, tab_id);
        let Some(tab) = s.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return Err((String::new(), "This AI tab's access has ended.".into()));
        };
        let label = tab.label.clone();
        if let Some(cutoff) = Instant::now().checked_sub(RATE_WINDOW) {
            while tab.asked.front().is_some_and(|t| *t < cutoff) {
                tab.asked.pop_front();
            }
        }
        if tab.asked.len() >= RATE_MAX {
            return Err((label, format!("Too many requests: at most {RATE_MAX} a minute. Wait a moment.")));
        }
        tab.asked.push_back(Instant::now());
        if waiting >= MAX_WAITING {
            return Err((label, format!("{MAX_WAITING} requests are already waiting for the user. Wait for them first.")));
        }
        Ok(label)
    }

    /// The servers a request on several names (`servers`, or every one
    /// shared with the AI tab when `all`), each once, in the order named:
    /// none, more than MAX_GROUP, or one not shared with it, is refused.
    fn named_servers(&self, tab_id: &str, servers: Vec<String>, all: bool) -> Result<Vec<String>, String> {
        let s = self.lock();
        let tab = s.tabs.iter().find(|t| t.id == tab_id).ok_or("This AI tab's access has ended.")?;
        let shared: Vec<String> = tab.servers.iter().map(|x| x.name.clone()).collect();
        let mut names: Vec<String> = if all { shared.clone() } else { servers };
        let mut seen = std::collections::HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        if names.is_empty() {
            return Err(if all || shared.is_empty() {
                "No servers are shared with you. Ask the user to click Share with AI on a server's tab.".to_string()
            } else {
                "Name the servers in servers, or set all to true.".to_string()
            });
        }
        if names.len() > MAX_GROUP {
            return Err(format!("At most {MAX_GROUP} servers in one request."));
        }
        let unknown: Vec<String> = names.iter().filter(|n| !shared.contains(n)).map(|n| for_log(n, MAX_LOGGED_NAME)).collect();
        if !unknown.is_empty() {
            return Err(format!(
                "Not shared with you: {}. Shared: {}. Ask the user to click Share with AI on its tab.",
                unknown.join(", "),
                if shared.is_empty() { "none".to_string() } else { shared.join(", ") }
            ));
        }
        Ok(names)
    }

    /// One command on several shared servers, or all of them (#158): one
    /// request, one card. It runs without asking only where every server's
    /// settings would run it; else every server asks, on the one card. The
    /// servers run side by side, and the AI gets one answer, a part each.
    async fn run_on_servers(&self, tab_id: &str, servers: Vec<String>, all: bool, command: &str, timeout: Option<u64>, cancel: watch::Receiver<bool>) -> ToolResult {
        let names = match self.named_servers(tab_id, servers, all) {
            Ok(names) => names,
            Err(why) => return ToolResult::err(why),
        };
        if command.trim().is_empty() {
            return ToolResult::err("No command was given.");
        }
        if let Err((label, why)) = self.admit_group(tab_id) {
            return self.refuse(&label, "", command, why);
        }
        // Whether every server would run it without asking.
        let mut ask = false;
        for name in &names {
            if self.early_decision(tab_id, name, command, false).await.decision.approved_by().is_none() {
                ask = true;
                break;
            }
        }
        let group = {
            let mut s = self.lock();
            s.next_group += 1;
            GroupRef { id: s.next_group, size: names.len(), ask }
        };
        let mut parts = tokio::task::JoinSet::new();
        for (i, name) in names.iter().enumerate() {
            let (me, tab, name, cmd, cancel) = (self.clone(), tab_id.to_string(), name.clone(), command.to_string(), cancel.clone());
            parts.spawn(async move { (i, me.run_command_in(&tab, &name, &cmd, timeout, false, cancel, Some(group)).await) });
        }
        let mut results: Vec<Option<ToolResult>> = (0..names.len()).map(|_| None).collect();
        while let Some(joined) = parts.join_next().await {
            if let Ok((i, r)) = joined {
                results[i] = Some(r);
            }
        }
        let results: Vec<(String, ToolResult)> =
            names.into_iter().zip(results).map(|(n, r)| (n, r.unwrap_or_else(|| ToolResult::err("It stopped unexpectedly.")))).collect();
        group_answer(&results)
    }

    /// "Allow everything here" ends for the shared servers `which` picks,
    /// in every AI tab. Returns how many it was on for.
    fn end_allow_all(&self, which: impl Fn(&SharedServer) -> bool, why: &str) -> usize {
        self.end_allow_all_where(|_, x| which(x), Some(why))
    }

    /// The same, `which` given the AI tab's id too; `why` for the log.
    fn end_allow_all_where(&self, which: impl Fn(&str, &SharedServer) -> bool, why: Option<&str>) -> usize {
        let mut ended = Vec::new();
        {
            let mut s = self.lock();
            for t in s.tabs.iter_mut() {
                let tab_id = t.id.clone();
                for x in t.servers.iter_mut().filter(|x| x.allow_all && which(&tab_id, x)) {
                    x.allow_all = false;
                    ended.push((t.label.clone(), x.name.clone()));
                }
            }
        }
        for (label, server) in &ended {
            let mut e = audit::Entry::new("allow_everything_ended", label);
            e.server = Some(server.clone());
            e.by = Some(BY_YOU);
            e.note = why.map(str::to_string);
            self.audit(e);
        }
        if !ended.is_empty() {
            self.changed();
        }
        ended.len()
    }

    /// End it for one server in one AI tab.
    fn end_allow_all_in(&self, tab_id: &str, session_id: &str) -> usize {
        self.end_allow_all_where(|t, x| t == tab_id && x.session_id == session_id, None)
    }

    /// The saved servers shared with any AI tab, by local id.
    fn shared_server_ids(&self) -> Vec<Option<i64>> {
        let mut ids: Vec<Option<i64>> = self.lock().tabs.iter().flat_map(|t| t.servers.iter().map(|x| x.server_id)).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// A line in the audit log for a change you made to the levels or
    /// rules.
    fn log_setting(&self, note: String) {
        let mut e = audit::Entry::new("approval_setting", "");
        e.by = Some(BY_YOU);
        e.note = Some(note);
        self.audit(e);
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        // Each shared server's level, for its marker: read before the lock,
        // as the vault has its own.
        let mut ids: Vec<Option<i64>> = self.lock().tabs.iter().flat_map(|t| t.servers.iter().map(|x| x.server_id)).collect();
        ids.sort();
        ids.dedup();
        let levels: Vec<(Option<i64>, u8, bool)> = ids
            .into_iter()
            .map(|id| {
                let p = (self.0.policy)(id);
                (id, p.level, p.reads_alone())
            })
            .collect();
        let level_of = |id: Option<i64>| levels.iter().find(|(i, ..)| *i == id).map(|(_, l, r)| (*l, *r)).unwrap_or((trust::ASK_EVERY_TIME, false));
        let s = self.lock();
        Snapshot {
            enabled: s.enabled,
            tabs: s
                .tabs
                .iter()
                .map(|t| TabView {
                    id: t.id.clone(),
                    label: t.label.clone(),
                    servers: t
                        .servers
                        .iter()
                        .map(|x| SharedServer {
                            level: level_of(x.server_id).0,
                            reads_alone: level_of(x.server_id).1,
                            root_via: x.root.as_ref().map(|k| k.0.tool),
                            root: None,
                            ..x.clone()
                        })
                        .collect(),
                    intruder_at: t.intruder_at.clone(),
                })
                .collect(),
            // A card as root asks for the password while this AI tab has no
            // password for root there: what another card's password found
            // since counts. Root found without a password hides the field
            // only on a card that hasn't asked yet: one the server asked a
            // password for (that root didn't do for this command) keeps it,
            // and a password typed there takes that root's place.
            pending: s
                .pending
                .iter()
                .map(|p| {
                    let mut v = p.view.clone();
                    if v.as_root && v.user.as_deref() != Some("root") {
                        match kept_in(&s, &v.tab_id, &v.session_id) {
                            Some(k) if k.0.password.is_some() || matches!(v.password, PasswordAsk::None | PasswordAsk::Optional) => {
                                v.password = PasswordAsk::None;
                                v.root_via = Some(k.0.tool);
                            }
                            Some(k) => v.root_via = v.root_via.or(Some(k.0.tool)),
                            None if v.password == PasswordAsk::None => v.password = PasswordAsk::Optional,
                            None => {}
                        }
                    }
                    v
                })
                .collect(),
            running: s.running.iter().map(follow::running_view).collect(),
            server_asks: s.server_asks.iter().map(|a| a.view.clone()).collect(),
            history: s.history.iter().rev().map(|d| d.view.clone()).collect(),
            audit_log: self.0.audit.path().map(|p| p.to_string_lossy().into_owned()),
        }
    }

    /// Keep a finished request's row, unless the history was cleared since
    /// it was asked (`epoch`, from then).
    fn remember(&self, epoch: u64, done: Done) {
        let mut s = self.lock();
        if s.epoch != epoch {
            return;
        }
        s.history.push_back(done);
        while s.history.len() > HISTORY {
            s.history.pop_front();
        }
    }

    // -- The channel and MCP --------------------------------------------------

    /// One connection from a helper: its hello first, then MCP for its tab.
    async fn serve(&self, stream: Box<dyn channel::Stream>) {
        use tokio::io::AsyncWriteExt;
        let (r, mut w) = tokio::io::split(stream);
        let mut r = tokio::io::BufReader::new(r);
        let hello = tokio::time::timeout(Duration::from_secs(5), channel::read_line(&mut r, 4096)).await;
        // The token's copies here are wiped when dropped: the line and the
        // parsed hello. (The reader's own buffer and the tool's environment
        // aren't ours to wipe.)
        let line = hello.ok().and_then(|l| l.ok().flatten()).map(Zeroizing::new);
        let token = line
            .as_deref()
            .and_then(|l| serde_json::from_str::<channel::Hello>(l).ok())
            .filter(|h| h.submarine_ai == channel::PROTOCOL)
            .map(|h| Zeroizing::new(h.token));
        drop(line);
        let tab = token.as_deref().and_then(|t| self.tab_for_token(t));
        drop(token);
        // One live connection per AI tab: a second one with its token is
        // another program that has read it (from the AI tool's environment,
        // say), and is refused.
        let unknown = "Submarine doesn't know this AI tab: its access has ended, or it wasn't started from Submarine's Home menu.";
        let (tab, refused) = match tab {
            None => (None, Some(unknown)),
            Some((tab_id, closed)) => match self.claim(&tab_id).await {
                Claim::Got(live) => (Some((tab_id, closed, live)), None),
                Claim::Gone => (None, Some(unknown)),
                Claim::Taken => {
                    self.intruder(&tab_id);
                    (None, Some("This AI tab is already connected to Submarine, and only one connection is allowed. Submarine has warned the user."))
                }
            },
        };
        let welcome = channel::Welcome { ok: refused.is_none(), error: refused.map(str::to_string) };
        let mut line = serde_json::to_string(&welcome).unwrap_or_default();
        line.push('\n');
        if w.write_all(line.as_bytes()).await.is_err() {
            return;
        }
        let _ = w.flush().await;
        let Some((tab_id, closed, live)) = tab else { return };
        mcp::serve(self.clone(), tab_id, closed, r, w, live).await;
    }

    async fn call_tool(&self, tab_id: &str, name: &str, args: &serde_json::Value, cancel: watch::Receiver<bool>) -> ToolResult {
        match name {
            "list_shared" => self.list_shared(tab_id).await,
            "run_command" => {
                let server = args.get("server").and_then(|v| v.as_str()).unwrap_or("");
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64());
                let as_root = match args.get("as_root") {
                    None | Some(serde_json::Value::Null) => false,
                    Some(serde_json::Value::Bool(b)) => *b,
                    Some(_) => return ToolResult::err("as_root must be true or false."),
                };
                self.run_command(tab_id, server, command, timeout, as_root, cancel).await
            }
            "run_on_servers" => {
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64());
                let servers = match args.get("servers") {
                    None | Some(serde_json::Value::Null) => Vec::new(),
                    Some(serde_json::Value::Array(a)) if a.iter().all(|v| v.is_string()) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                    Some(_) => return ToolResult::err("servers must be a list of server names."),
                };
                let all = match args.get("all") {
                    None | Some(serde_json::Value::Null) => false,
                    Some(serde_json::Value::Bool(b)) => *b,
                    Some(_) => return ToolResult::err("all must be true or false."),
                };
                self.run_on_servers(tab_id, servers, all, command, timeout, cancel).await
            }
            "copy_files" => self.copy_files(tab_id, args, cancel).await,
            "request_server" => self.request_server(tab_id, args, cancel).await,
            "follow" | "read_follow" | "stop_follow" => self.follow_tool(tab_id, name, args, cancel).await,
            _ => ToolResult::err(format!("Submarine has no tool called {name}.")),
        }
    }

    /// What the AI may use, told up front (#176): each shared server and
    /// what happens to its requests there, the folders it may pull into,
    /// and the limits. Never which rules a server has, or who shared it.
    /// The names are the user's, not a server's, so they're not tagged as
    /// server output.
    async fn list_shared(&self, tab_id: &str) -> ToolResult {
        let shared: Vec<(String, Option<i64>, bool)> = {
            let s = self.lock();
            let Some(tab) = s.tabs.iter().find(|t| t.id == tab_id) else {
                return ToolResult::err("This AI tab's access has ended.");
            };
            if tab.servers.is_empty() {
                return ToolResult::ok(
                    "No servers are shared with you yet. Ask the user to open the server's tab in Submarine and click Share with AI on its toolbar.",
                );
            }
            tab.servers.iter().map(|x| (x.name.clone(), x.server_id, x.allow_all)).collect()
        };
        // Each saved server's settings read once, as early_decision reads
        // them: off the async workers, as the vault has a lock of its own.
        // Should that fail, every server asks.
        let requests: Vec<(trust::Requests, Option<String>)> = {
            let lookup = self.0.policy.clone();
            let each: Vec<(Option<i64>, bool)> = shared.iter().map(|(_, id, all)| (*id, *all)).collect();
            let n = each.len();
            tokio::task::spawn_blocking(move || {
                let mut read: Vec<(Option<i64>, trust::Policy)> = Vec::new();
                each.into_iter()
                    .map(|(id, allow_all)| {
                        let at = match read.iter().position(|(i, _)| *i == id) {
                            Some(at) => at,
                            None => {
                                read.push((id, lookup(id)));
                                read.len() - 1
                            }
                        };
                        (trust::Requests::of(&read[at].1, allow_all), read[at].1.os.clone().filter(|os| trust::is_os_id(os)))
                    })
                    .collect()
            })
            .await
            .unwrap_or_else(|_| vec![(trust::Requests::AskEachTime, None); n])
        };
        let servers = shared.iter().zip(requests).map(|((name, _, _), (r, os))| ListedServer { name, os, requests: r.phrase() }).collect();
        let copy_into = self.pull_folders().iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let listed = Listed {
            servers,
            copy_into,
            limits: ListedLimits {
                files_per_copy: copy::MAX_FILES,
                bytes_per_copy: copy::MAX_BYTES,
                servers_per_request: MAX_GROUP,
                timeout_seconds: ListedTimeout { default: DEFAULT_TIMEOUT, max: MAX_TIMEOUT },
                output_bytes_to_you: AI_HEAD + AI_TAIL,
                follows_per_tab: follow::MAX_FOLLOWS,
                follow_buffer_bytes: follow::RING,
            },
        };
        match serde_json::to_string(&listed) {
            Ok(text) => ToolResult::ok(text),
            Err(e) => ToolResult::err(format!("Submarine couldn't list the shared servers: {e}")),
        }
    }

    /// Refused before it reaches you: logged, and the AI told why.
    fn refuse(&self, label: &str, server: &str, command: &str, why: String) -> ToolResult {
        self.refuse_as("run_command", label, server, command, why)
    }

    /// The same, for the tool `request`.
    fn refuse_as(&self, request: &'static str, label: &str, server: &str, command: &str, why: String) -> ToolResult {
        let mut e = audit::Entry::new("request", label);
        e.request = Some(request);
        e.server = Some(for_log(server, MAX_LOGGED_NAME)).filter(|s| !s.is_empty());
        e.command = Some(for_log(command, MAX_LOGGED_COMMAND));
        e.decision = Some("refused");
        e.by = Some("Submarine");
        e.note = Some(why.clone());
        self.audit(e);
        ToolResult::err(why)
    }

    async fn run_command(
        &self,
        tab_id: &str,
        server: &str,
        command: &str,
        timeout: Option<u64>,
        as_root: bool,
        cancel: watch::Receiver<bool>,
    ) -> ToolResult {
        self.run_command_in(tab_id, server, command, timeout, as_root, cancel, None).await.for_ai()
    }

    /// What a server's settings say of `command` there, before it's asked:
    /// the share they were read for (its saved id and "allow everything"),
    /// whether the command asks for root or opens a shell, the settings, and
    /// the decision.
    async fn early_decision(&self, tab_id: &str, server: &str, command: &str, as_root: bool) -> Early {
        // The named server's settings, read before the lock (the vault has
        // its own) and checked again under it.
        let looked_up = {
            let s = self.lock();
            s.tabs.iter().find(|t| t.id == tab_id).and_then(|t| t.servers.iter().find(|x| x.name == server)).map(|x| (x.server_id, x.allow_all))
        };
        // The command read once, the settings read and the decision made,
        // outside the lock and off the async workers (the vault has a lock
        // of its own, and the AI writes the command). A command too long to
        // run is refused below, unread. Should it fail, it asks, as root.
        let (root_cmd, opens_shell, policy, early) = {
            let lookup = self.0.policy.clone();
            let cmd = command.to_string();
            tokio::task::spawn_blocking(move || {
                let parsed = (cmd.len() <= MAX_COMMAND && !cmd.contains('\0')).then(|| trust::Parsed::new(&cmd));
                let policy = looked_up.map(|(id, _)| lookup(id));
                let early = match (&policy, &parsed, looked_up) {
                    (Some(p), Some(c), Some((_, allow_all))) => trust::decide_parsed(p, allow_all, c, as_root),
                    _ => trust::Decision::Ask { rule: None },
                };
                (parsed.as_ref().is_some_and(|p| p.root), parsed.as_ref().is_some_and(|p| p.opens_shell()), policy, early)
            })
            .await
            .unwrap_or_else(|_| (true, false, Some(trust::Policy::unreadable()), trust::Decision::Ask { rule: None }))
        };
        Early { looked_up, root_cmd, opens_shell, policy, decision: early }
    }

    /// A request's checks, before anything is shown to you or runs: the
    /// rate, its arguments, the server, root, how many run or wait, and
    /// for a follow (#179) how many are open. Passed, it's on the waiting
    /// list with its card, or (allowed by your settings) on the running
    /// list with its stop signal, in the same step. `run_command`, each
    /// server of `run_on_servers` (`group`, counted against the limits as a
    /// whole) and `follow` all come through here.
    #[allow(clippy::too_many_arguments)]
    async fn admit(
        &self,
        tab_id: &str,
        server: &str,
        command: &str,
        timeout: Option<u64>,
        as_root: bool,
        group: Option<GroupRef>,
        kind: Kind,
    ) -> Result<Admitted, ToolResult> {
        let Early { looked_up, root_cmd, opens_shell, policy, decision: early } = self.early_decision(tab_id, server, command, as_root).await;
        let follows = kind == Kind::Follow;
        // How the log names it: refused, it's named as before (#179 adds
        // `follow`).
        let request = if follows {
            "follow"
        } else if group.is_some() {
            "run_on_servers"
        } else {
            "run_command"
        };
        let refused_as = if follows { "follow" } else { "run_command" };
        let admitted = {
            let mut s = self.lock();
            let next_id = s.next_id + 1;
            let epoch = s.epoch;
            let waiting = waiting_requests(&s, tab_id);
            let open_follows = follow::open_in(&s, tab_id);
            let Some(tab) = s.tabs.iter_mut().find(|t| t.id == tab_id) else {
                return Err(ToolResult::err("This AI tab's access has ended."));
            };
            let label = tab.label.clone();
            // Every call counts toward the rate limit, refused or not; past
            // it, one line in the log a window, not one a call.
            if let Some(cutoff) = Instant::now().checked_sub(RATE_WINDOW) {
                while tab.asked.front().is_some_and(|t| *t < cutoff) {
                    tab.asked.pop_front();
                }
            }
            // A group's servers were counted as one request (admit_group).
            if group.is_none() && tab.asked.len() >= RATE_MAX {
                let log = tab.rate_logged.is_none_or(|t| t.elapsed() >= RATE_WINDOW);
                if log {
                    tab.rate_logged = Some(Instant::now());
                }
                drop(s);
                let why = format!("Too many requests: at most {RATE_MAX} a minute. Wait a moment.");
                if !log {
                    return Err(ToolResult::err(why));
                }
                return Err(self.refuse_as(refused_as, &label, server, command, why));
            }
            if group.is_none() {
                tab.asked.push_back(Instant::now());
            }
            let refuse = |why: String| (label.clone(), why);
            let checked: Result<SharedServer, (String, String)> = (|| {
                if command.trim().is_empty() {
                    return Err(refuse("No command was given.".into()));
                }
                if command.len() > MAX_COMMAND || command.contains('\0') {
                    return Err(refuse(format!("The command is longer than {} KB, or has a NUL character.", MAX_COMMAND / 1024)));
                }
                if timeout.is_some_and(|t| t == 0 || t > MAX_TIMEOUT) {
                    return Err(refuse(format!("timeout_seconds must be from 1 to {MAX_TIMEOUT}.")));
                }
                let Some(found) = tab.servers.iter().find(|x| x.name == server) else {
                    let names: Vec<&str> = tab.servers.iter().map(|x| x.name.as_str()).collect();
                    // The name as the AI sent it, cut: it may be a megabyte.
                    let asked = for_log(server, MAX_LOGGED_NAME);
                    return Err(refuse(if names.is_empty() {
                        format!("{asked} isn't shared with you, and no server is. Ask the user to click Share with AI on its tab.")
                    } else {
                        format!("{asked} isn't shared with you. Shared: {}. Ask the user to click Share with AI on its tab.", names.join(", "))
                    }));
                };
                if as_root && opens_shell {
                    return Err(refuse(
                        "as_root runs one command, with no terminal and nothing on stdin, so it can't open a root shell, \
                         and takes no options for sudo or dzdo. Send the command itself (such as `docker ps -a`), and Submarine runs it as root."
                            .into(),
                    ));
                }
                if as_root && policy.as_ref().is_some_and(|p| p.windows) {
                    return Err(refuse(format!("{} is a Windows server: as_root works with sudo or dzdo, on Linux and other Unix servers.", found.name)));
                }
                if follows && open_follows >= follow::MAX_FOLLOWS {
                    return Err(refuse(format!(
                        "{} follows are already open in this AI tab. Stop one with stop_follow, or read an ended one to the end, first.",
                        follow::MAX_FOLLOWS
                    )));
                }
                Ok(found.clone())
            })();
            let found = match checked {
                Ok(f) => f,
                Err((label, why)) => {
                    drop(s);
                    return Err(self.refuse_as(refused_as, &label, server, command, why));
                }
            };
            // Whether it waits for you (trust.rs), decided for this same
            // share as it is now (not one shared again meanwhile, or whose
            // "allow everything" changed): else it asks.
            let decision = if looked_up == Some((found.server_id, found.allow_all)) { early } else { trust::Decision::Ask { rule: None } };
            // Another server of the same request asks, so this one does too.
            let held_for_group = group.is_some_and(|g| g.ask) && decision.approved_by().is_some();
            let decision = if held_for_group { trust::Decision::Ask { rule: None } } else { decision };
            let auto = decision.approved_by();
            if running_on(&s, tab_id, &found.session_id) >= MAX_RUNNING {
                drop(s);
                return Err(self.refuse_as(
                    refused_as,
                    &label,
                    server,
                    command,
                    format!("{MAX_RUNNING} of your commands are already running on {}. Wait for one to finish.", found.name),
                ));
            }
            if auto.is_none() && group.is_none() && waiting >= MAX_WAITING {
                drop(s);
                return Err(self.refuse_as(
                    refused_as,
                    &label,
                    server,
                    command,
                    format!("{MAX_WAITING} requests are already waiting for the user. Wait for them first."),
                ));
            }
            s.next_id = next_id;
            let (stop_tx, stop_rx) = watch::channel(false);
            let login_root = found.user.as_deref() == Some("root");
            let ask_rule = match &decision {
                trust::Decision::Ask { rule } => rule.clone(),
                _ => None,
            };
            // Its card, as it's shown now or if it comes back to ask for the
            // root password.
            let card = PendingView {
                id: next_id,
                tab_id: tab_id.to_string(),
                tab_label: label.clone(),
                session_id: found.session_id.clone(),
                server: found.name.clone(),
                user: found.user.clone(),
                command: command.to_string(),
                root: login_root || root_cmd || as_root,
                as_root,
                root_via: found.root.as_ref().map(|k| k.0.tool),
                // A tab logged in as root runs it as it is: nothing to ask.
                password: if as_root && !login_root && found.root.is_none() { PasswordAsk::Optional } else { PasswordAsk::None },
                timeout_secs: if follows { follow::LIFE.as_secs() } else { timeout.unwrap_or(DEFAULT_TIMEOUT) },
                asked_at: now(),
                server_id: found.server_id,
                can_allow_always: found.server_id.is_some()
                    && ask_rule.is_none()
                    && !root_cmd
                    && !as_root
                    && policy.as_ref().is_some_and(|p| p.force_ask.is_none())
                    && trust::is_simple_on(command, policy.as_ref().is_some_and(|p| p.windows)),
                note: policy
                    .as_ref()
                    .and_then(|p| p.force_ask.clone())
                    .or_else(|| held_for_group.then(|| "This server's settings would run it, but others in this request ask, so it asks with them.".to_string()))
                    .filter(|_| ask_rule.is_none()),
                ask_rule,
                group: group.map(|g| GroupView { id: g.id, size: g.size }),
                copy: None,
                follow: follows,
            };
            let rx = match &auto {
                // Runs now: in the running list with its stop signal, in
                // this same step, so any end of access stops it.
                Some(by) => {
                    s.running.push(Running {
                        view: RunningView {
                            id: next_id,
                            tab_label: label.clone(),
                            session_id: found.session_id.clone(),
                            server: found.name.clone(),
                            command: command.to_string(),
                            started_at: now(),
                            ran_without_asking: Some(by.clone()),
                            as_root,
                            root_via: card.root_via,
                            follow: follows.then(follow::FollowView::default),
                        },
                        tab_id: tab_id.to_string(),
                        session_id: found.session_id.clone(),
                        stop: stop_tx.clone(),
                        feed: None,
                    });
                    None
                }
                None => {
                    let (tx, rx) = oneshot::channel();
                    s.pending.push(Pending { view: card.clone(), decide: tx, stop: stop_tx.clone() });
                    Some(rx)
                }
            };
            let req = Req {
                tab_id: tab_id.to_string(),
                session_id: found.session_id,
                label,
                server: found.name,
                user: found.user,
                command: command.to_string(),
                id: next_id,
                epoch,
                request,
                root: card.root,
                as_root,
            };
            Admitted { req, rx, auto, stop_tx, stop_rx, card }
        };
        self.changed();
        if admitted.rx.is_some() && self.0.flash.load(Ordering::Relaxed) {
            (self.0.attention)();
        }
        Ok(admitted)
    }

    /// Your answer to a request waiting for you: Allow (how, and a root
    /// password you typed on its card), or what the AI is told when it's
    /// denied, cancelled by an end of access, or withdrawn by the AI first,
    /// each logged and kept in the history. `asked`: what its card asks of
    /// the password, for what the AI is told when it's denied.
    async fn await_answer(
        &self,
        req: &Req,
        mut rx: oneshot::Receiver<Answer>,
        cancel: &mut watch::Receiver<bool>,
        asked: Asked,
    ) -> Result<(String, Option<Zeroizing<String>>), ToolResult> {
        // The answer: Some, or None when access ended first (the request
        // dropped by cancel_where or answer).
        let decision: Result<Option<Answer>, ()> = tokio::select! {
            biased;
            d = &mut rx => Ok(d.ok()),
            _ = withdrawal(cancel) => Err(()),
        };
        let decision = match decision {
            Ok(d) => d,
            // The AI withdrew it: unless you or an end of access got to it
            // first, it's taken off the waiting list here.
            Err(()) => {
                let was_waiting = {
                    let mut s = self.lock();
                    let before = s.pending.len();
                    s.pending.retain(|p| p.view.id != req.id);
                    before != s.pending.len()
                };
                if was_waiting {
                    let mut e = req.entry("request");
                    e.decision = Some("cancelled");
                    e.note = Some("cancelled by the AI tool".into());
                    self.audit(e);
                    self.remember(req.epoch, req.done("cancelled"));
                    self.changed();
                    return Err(ToolResult::err("Cancelled before the user decided: the request was withdrawn."));
                }
                // Taken off under the same lock as the answer was given, so
                // it's there now (or the request was dropped).
                rx.try_recv().ok()
            }
        };
        match decision {
            None => {
                let mut e = req.entry("request");
                e.decision = Some("cancelled");
                e.note = Some("access ended".into());
                self.audit(e);
                self.remember(req.epoch, req.done("cancelled"));
                self.changed();
                Err(ToolResult::err("Cancelled before the user decided: the server isn't shared any more."))
            }
            Some(Answer::Deny(note)) => {
                let mut e = req.entry("request");
                e.decision = Some("denied");
                e.by = Some(BY_YOU);
                e.note = note.as_ref().map(|n| format!("note: {n}"));
                self.audit(e);
                self.remember(req.epoch, req.done("denied"));
                self.changed();
                let after = asked.for_ai(&req.server);
                Err(ToolResult::err(match note {
                    Some(n) => format!("Denied by the user: {n}{after}"),
                    None => format!("Denied by the user.{after}"),
                }))
            }
            Some(Answer::Allow { by, password }) => Ok((by, password)),
        }
    }

    /// Logged as it's allowed: a long command, or one that hangs (or a
    /// follow), is in the log while it runs, not only once it ends. Its
    /// "finished" line follows, with the same id.
    fn log_allowed(&self, req: &Req, approved_by: &str, round: u32) {
        let mut e = req.entry("request");
        e.decision = Some("allowed");
        e.by = Some(if approved_by != BY_YOU { "your settings" } else { BY_YOU });
        e.approved_by = Some(approved_by.to_string());
        if round > 1 {
            e.note = Some("allowed again, after the server asked for the login's password".into());
        }
        self.audit(e);
    }

    /// `run_command`, or one server's part of a `run_on_servers` request
    /// (`group`), which was counted against the limits as a whole.
    #[allow(clippy::too_many_arguments)]
    async fn run_command_in(
        &self,
        tab_id: &str,
        server: &str,
        command: &str,
        timeout: Option<u64>,
        as_root: bool,
        mut cancel: watch::Receiver<bool>,
        group: Option<GroupRef>,
    ) -> ToolResult {
        let Admitted { req, rx, auto, stop_tx, stop_rx, card } = match self.admit(tab_id, server, command, timeout, as_root, group, Kind::Once).await {
            Ok(a) => a,
            Err(refused) => return refused,
        };
        let (session_id, server, user, id, epoch) = (&req.session_id, &req.server, &req.user, req.id, req.epoch);
        // As root: the login, the command (read above, not again), or the
        // AI's asking. Through sudo or dzdo only where the login isn't root.
        let via_tool = as_root && user.as_deref() != Some("root");
        let limit = Duration::from_secs(timeout.unwrap_or(DEFAULT_TIMEOUT));
        let entry_for = |event: &'static str| req.entry(event);
        let done = |outcome: &'static str| req.done(outcome);
        let withdrawn = |c: &watch::Receiver<bool>| *c.borrow();

        // Allowed (at once, `auto`, or by your answer), then run. A request
        // as root whose server wants a password it didn't have, or turned
        // down the one you typed, comes back to you with its card asking for
        // it, and is allowed again: so this goes round until it runs, or you
        // deny it, or access ends.
        let mut auto = auto;
        let mut rx = rx;
        // What its card asks of the password, once root wanted one.
        let mut asked = Asked::Nothing;
        let mut round = 0u32;
        let (approved_by, result, via) = loop {
            round += 1;
            let (approved_by, typed) = match (auto.take(), rx.take()) {
                (Some(by), _) => (by, None),
                (None, None) => return ToolResult::err("It stopped unexpectedly."),
                (None, Some(rx)) => match self.await_answer(&req, rx, &mut cancel, asked).await {
                    Ok(answer) => answer,
                    Err(answer) => return answer,
                },
            };
            self.log_allowed(&req, &approved_by, round);

            // Allowed: answer() (or the step above, when it runs without asking)
            // has put it in the running list with its stop signal, which an end
            // of access sets. The AI withdrawing it sets it
            // too. Either way exec::run ends it on the server before returning;
            // it runs as a task of its own, so it does even if this is dropped.
            if withdrawn(&cancel) {
                let _ = stop_tx.send(true);
            }
            if !via_tool {
                let mut task = tokio::spawn((self.0.exec)(session_id.clone(), command.to_string(), limit, stop_rx.clone()));
                let joined = tokio::select! {
                    r = &mut task => r,
                    _ = withdrawal(&mut cancel) => {
                        let _ = stop_tx.send(true);
                        task.await
                    }
                };
                let result = joined.unwrap_or_else(|e| Err(format!("It stopped unexpectedly: {e}")));
                break (approved_by, result.map_err(|e| (e.clone(), format!("Couldn't run it: {e}"))), None);
            }

            // As root (#120): with the password you just typed, else with
            // what this AI tab has for root there, else looking for root
            // that needs no password.
            let kept = self.kept_root(tab_id, session_id).filter(|_| typed.is_none());
            let how = match (typed, &kept) {
                (Some(pw), _) => exec::RootHow::Typed(pw),
                (None, Some(k)) => exec::RootHow::Kept(k.0.clone()),
                (None, None) => exec::RootHow::Find,
            };
            // A kept password is tried by one request at a time until the
            // tool has taken it, so one that stopped working (changed on the
            // server) costs one failed attempt, not one per request. A
            // request that waited finds it forgotten, and asks you.
            let mut turn = None;
            let mut gone = false;
            if let Some(k) = kept.as_ref().filter(|k| k.0.password.is_some()) {
                let mut stop_wait = stop_rx.clone();
                turn = tokio::select! {
                    g = k.0.checked.clone().lock_owned() => Some(g),
                    _ = stop_wait.wait_for(|s| *s) => None,
                    _ = withdrawal(&mut cancel) => {
                        let _ = stop_tx.send(true);
                        None
                    }
                };
                gone = turn.is_some() && !self.kept_root(tab_id, session_id).is_some_and(|x| Arc::ptr_eq(&x.0, &k.0));
            }
            let run = if kept.as_ref().is_some_and(|k| k.0.password.is_some()) && turn.is_none() {
                exec::RootRun::Ran(exec::Exec { stopped: true, ..exec::Exec::default() }, None)
            } else if gone {
                exec::RootRun::NeedPassword(kept.as_ref().map(|k| k.0.tool))
            } else {
                let (started_tx, mut started_rx) = oneshot::channel();
                let job = RootJob { command: command.to_string(), how, started: Some(started_tx) };
                let mut task = tokio::spawn((self.0.root_exec)(session_id.clone(), job, limit, stop_rx.clone()));
                let mut watching = turn.is_some();
                let mut withdrawn_seen = false;
                let joined = loop {
                    tokio::select! {
                        biased;
                        r = &mut task => break r,
                        s = &mut started_rx, if watching => {
                            watching = false;
                            // The tool took it: the next request may go.
                            if s.is_ok() {
                                turn = None;
                            }
                        }
                        _ = withdrawal(&mut cancel), if !withdrawn_seen => {
                            withdrawn_seen = true;
                            let _ = stop_tx.send(true);
                        }
                    }
                };
                joined.unwrap_or_else(|e| {
                    let why = format!("It stopped unexpectedly: {e}");
                    exec::RootRun::Failed { you: why.clone(), ai: format!("Couldn't run it: {why}") }
                })
            };
            let kept_tool = kept.as_ref().map(|k| k.0.tool);
            match run {
                exec::RootRun::Ran(out, new) => {
                    let tool = new.as_ref().map(|a| a.tool).or(kept_tool);
                    if let Some(a) = new {
                        self.keep_root(id, tab_id, session_id, a);
                    }
                    break (approved_by, Ok(out), tool);
                }
                exec::RootRun::Failed { you, ai } => {
                    // A kept password is never left in place after a failure:
                    // the next request asks for it again rather than risk it.
                    if let Some(k) = kept.as_ref().filter(|k| k.0.password.is_some()) {
                        self.forget_root(tab_id, session_id, &k.0);
                    }
                    break (approved_by, Err((you, ai)), kept_tool);
                }
                exec::RootRun::NeedPassword(tool) => {
                    drop(turn);
                    asked = Asked::Password(tool.or(kept_tool));
                    let mut e = entry_for("root_password");
                    e.root_tool = tool.or(kept_tool);
                    e.note = Some("the server asked for the login's password".into());
                    self.audit(e);
                    match self.ask_again(&card, PasswordAsk::Needed, tool.or(kept_tool), &stop_tx) {
                        Some(again) => rx = Some(again),
                        None => break (approved_by, Ok(exec::Exec { stopped: true, ..exec::Exec::default() }), tool),
                    }
                }
                exec::RootRun::WrongPassword(tool) => {
                    // A kept one turned down is forgotten at once: no other
                    // request tries it.
                    if let Some(k) = &kept {
                        self.forget_root(tab_id, session_id, &k.0);
                    }
                    drop(turn);
                    asked = Asked::Refused(tool.or(kept_tool));
                    let mut e = entry_for("root_password");
                    e.root_tool = tool.or(kept_tool);
                    e.note = Some("the password was refused (one attempt)".into());
                    self.audit(e);
                    match self.ask_again(&card, PasswordAsk::Wrong, tool.or(kept_tool), &stop_tx) {
                        Some(again) => rx = Some(again),
                        None => break (approved_by, Ok(exec::Exec { stopped: true, ..exec::Exec::default() }), tool),
                    }
                }
            }
            self.changed();
            if self.0.flash.load(Ordering::Relaxed) {
                (self.0.attention)();
            }
        };
        let ran_without_asking = Some(approved_by).filter(|b| b != BY_YOU);
        drop(stop_rx);

        // Still listed: nothing ended its access while it ran.
        let still_listed = {
            let mut s = self.lock();
            let before = s.running.len();
            s.running.retain(|r| r.view.id != id);
            before != s.running.len()
        };
        let stopped_by_ai = still_listed && withdrawn(&cancel);

        let mut e = entry_for("finished");
        e.root_tool = via;
        let mut d = done("done");
        d.view.ran_without_asking = ran_without_asking;
        d.view.root_via = via;
        let answer = match result {
            Ok(out) if out.stopped => {
                e.note = Some(if stopped_by_ai { "stopped: withdrawn by the AI tool" } else { "stopped: access ended" }.into());
                d.view.outcome = "stopped";
                ToolResult::err(if stopped_by_ai {
                    "Stopped: the request was withdrawn."
                } else {
                    "Stopped: the server isn't shared any more."
                })
            }
            Ok(mut out) => {
                // Up to a megabyte through every rule: off the async workers.
                // The main copies of it are wiped when they're dropped (not
                // every one the filter makes on the way).
                let output = Zeroizing::new(std::mem::take(&mut out.output));
                let output_bytes = output.len();
                let filtered = tokio::task::spawn_blocking(move || {
                    let text = Zeroizing::new(String::from_utf8_lossy(&output).into_owned());
                    let f = filter::filter(&text);
                    let filtered = Zeroizing::new(f.text);
                    let ai = Zeroizing::new(shorten_for_ai(&filtered));
                    (text, f.hidden, ai)
                })
                .await;
                let Ok((text, hidden, ai)) = filtered else {
                    e.note = Some("filtering the output failed".into());
                    d.view.outcome = "failed";
                    d.view.error = Some("Submarine couldn't filter the output, so the AI wasn't given it.".into());
                    self.audit(e);
                    self.remember(epoch, d);
                    self.changed();
                    return ToolResult::err("Submarine couldn't filter the output, so it isn't given.");
                };
                let exit = out.exit_status.map(i64::from);
                let mut head = match (exit, &out.exit_signal) {
                    (Some(c), _) => format!("Exit code: {c}\n"),
                    (None, Some(sig)) => format!("Ended by signal {sig}\n"),
                    (None, None) => "Exit code: unknown\n".to_string(),
                };
                if let Some(tool) = via {
                    head.push_str(&format!("Ran as root, through {tool}.\n"));
                }
                if out.timed_out {
                    head.push_str(&format!("Stopped by Submarine after {} seconds.\n", limit.as_secs()));
                }
                if out.cut {
                    head.push_str(&format!("Stopped by Submarine after {} MB of output.\n", OUTPUT_CAP / (1024 * 1024)));
                }
                if hidden > 0 {
                    head.push_str(&format!(
                        "Submarine hid {} item{} that looked like secrets (shown as {}).\n",
                        hidden,
                        if hidden == 1 { "" } else { "s" },
                        filter::HIDDEN
                    ));
                }
                // What the server printed goes after Submarine's lines, and
                // is marked as the server's as it's given (boundary.rs).
                let answer = ToolResult {
                    text: head.trim_end().to_string(),
                    is_error: false,
                    server_text: Some(boundary::ServerText { server: server.clone(), text: ai.to_string() }),
                };
                e.exit_code = exit;
                e.bytes_returned = Some(ai.len());
                e.items_hidden = Some(hidden);
                if out.timed_out {
                    e.note = Some("timed out".into());
                } else if out.cut {
                    e.note = Some("output cap reached".into());
                }
                d.view.exit_code = exit;
                d.view.output_bytes = output_bytes;
                d.output = text;
                d.ai_output = ai;
                d.view.hidden = hidden;
                d.view.cut = out.cut;
                d.view.timed_out = out.timed_out;
                answer
            }
            Err((err, for_ai)) => {
                e.note = Some(err.clone());
                d.view.outcome = "failed";
                d.view.error = Some(err);
                ToolResult::err(for_ai)
            }
        };
        self.audit(e);
        self.remember(epoch, d);
        self.changed();
        answer
    }

    // -- Root for the AI (#120) -------------------------------------------

    /// What `tab_id` has for root on `session_id`, if anything.
    fn kept_root(&self, tab_id: &str, session_id: &str) -> Option<KeptRoot> {
        kept_in(&self.lock(), tab_id, session_id)
    }

    /// Keep `access` for root on `session_id` in `tab_id`'s list: only while
    /// the request `id` is still running there, so not in a share that
    /// ended (and maybe started again) meanwhile.
    fn keep_root(&self, id: u64, tab_id: &str, session_id: &str, access: Arc<crate::ssh_manager::RootAccess>) {
        let mut s = self.lock();
        if !s.running.iter().any(|r| r.view.id == id) {
            return;
        }
        if let Some(x) = s.tabs.iter_mut().find(|t| t.id == tab_id).and_then(|t| t.servers.iter_mut().find(|x| x.session_id == session_id)) {
            x.root = Some(KeptRoot(access));
        }
    }

    /// Forget `access`, a password the tool turned down, if it's still the
    /// one kept.
    fn forget_root(&self, tab_id: &str, session_id: &str, access: &Arc<crate::ssh_manager::RootAccess>) {
        let mut s = self.lock();
        if let Some(x) = s.tabs.iter_mut().find(|t| t.id == tab_id).and_then(|t| t.servers.iter_mut().find(|x| x.session_id == session_id)) {
            if x.root.as_ref().is_some_and(|k| Arc::ptr_eq(&k.0, access)) {
                x.root = None;
            }
        }
    }

    /// A request as root that you (or the server's level) allowed, whose
    /// server wants a password it didn't have, or turned down the one
    /// given: back on the waiting list, its card asking for the password
    /// (`password`), in the same step as it leaves the running list. None
    /// when it's been stopped (withdrawn, or access ended) meanwhile.
    fn ask_again(&self, card: &PendingView, password: PasswordAsk, via: Option<&'static str>, stop: &watch::Sender<bool>) -> Option<oneshot::Receiver<Answer>> {
        let mut s = self.lock();
        if *stop.borrow() {
            return None;
        }
        let at = s.running.iter().position(|r| r.view.id == card.id)?;
        let shared = s.tabs.iter().any(|t| t.id == card.tab_id && t.servers.iter().any(|x| x.session_id == card.session_id));
        if !shared {
            return None;
        }
        s.running.remove(at);
        let (tx, rx) = oneshot::channel();
        let mut view = card.clone();
        view.password = password;
        view.root_via = via.or(view.root_via);
        view.asked_at = now();
        s.pending.push(Pending { view, decide: tx, stop: stop.clone() });
        Some(rx)
    }
}

/// What `tab_id` has for root on `session_id`, in `s`.
fn kept_in(s: &Shared, tab_id: &str, session_id: &str) -> Option<KeptRoot> {
    s.tabs.iter().find(|t| t.id == tab_id)?.servers.iter().find(|x| x.session_id == session_id)?.root.clone()
}

/// A root password typed on a card, as it's kept: wiped when dropped, None
/// when empty. One with a line break (or a NUL) is refused: it would end
/// the line the tool reads.
fn typed_password(password: Option<String>) -> Result<Option<Zeroizing<String>>, String> {
    let password = password.map(Zeroizing::new).filter(|p| !p.is_empty());
    if password.as_ref().is_some_and(|p| p.contains(['\n', '\r', '\0'])) {
        return Err("A password can't have a line break.".into());
    }
    Ok(password)
}

/// A root executor for tests that run nothing as root.
#[cfg(test)]
fn no_root() -> RootExecutor {
    Arc::new(|_, _, _, _| {
        Box::pin(async { exec::RootRun::Failed { you: "no root in this test".into(), ai: "Couldn't run it: no root in this test".into() } })
    })
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AiSettings {
    enabled: bool,
    /// Flash the taskbar when the AI needs you.
    flash: bool,
    /// Each AI tool Home's menu knows (tools.rs).
    tools: Vec<ToolView>,
    /// The folders `copy_files` may pull into (#159).
    pull_folders: Vec<String>,
    available: bool,
}

#[derive(Serialize)]
struct ToolView {
    id: &'static str,
    label: &'static str,
    /// Where it is, when it's installed.
    path: Option<String>,
}

fn settings_view(state: &AiState) -> AiSettings {
    AiSettings {
        enabled: state.enabled(),
        flash: state.0.flash.load(Ordering::Relaxed),
        tools: tools::TOOLS.iter().map(|t| ToolView { id: t.id, label: t.label, path: t.find().map(|p| p.to_string_lossy().into_owned()) }).collect(),
        pull_folders: state.pull_folders().iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        available: cfg!(not(target_os = "android")),
    }
}

/// The AI tools settings file, with what holds now and `enabled` and
/// `flash` as given.
fn save_settings(app: &tauri::AppHandle, state: &AiState, enabled: bool, flash: bool) -> Result<(), String> {
    let pull_folders = state.pull_folders().iter().map(|p| p.to_string_lossy().into_owned()).collect();
    crate::local_open::write_setting(app, SETTINGS_FILE, &Settings { enabled, flash, pull_folders }, "the AI tools setting")
}

#[tauri::command]
pub async fn ai_settings(state: tauri::State<'_, AiState>) -> Result<AiSettings, String> {
    Ok(settings_view(&state))
}

#[tauri::command]
pub async fn ai_set_enabled(app: tauri::AppHandle, state: tauri::State<'_, AiState>, enabled: bool) -> Result<AiSettings, String> {
    if cfg!(target_os = "android") {
        return Err("AI tools aren't available on Android.".into());
    }
    let flash = state.0.flash.load(Ordering::Relaxed);
    save_settings(&app, &state, enabled, flash)?;
    if enabled {
        state.turn_on()?;
    } else {
        state.turn_off().await;
    }
    Ok(settings_view(&state))
}

/// "Flash the taskbar when the AI needs you", on this device.
#[tauri::command]
pub async fn ai_set_flash(app: tauri::AppHandle, state: tauri::State<'_, AiState>, flash: bool) -> Result<AiSettings, String> {
    let enabled = state.enabled();
    save_settings(&app, &state, enabled, flash)?;
    state.0.flash.store(flash, Ordering::Relaxed);
    Ok(settings_view(&state))
}

/// "Add a folder…" under "Folders the AI may copy into" (#159): the
/// system's folder picker, then the folder it gives is added, once checked
/// (copy::allowable_folder). Closed without a pick, nothing changes. The
/// page never names the folder, and no MCP tool reaches this.
#[tauri::command]
pub async fn ai_add_pull_folder(app: tauri::AppHandle, state: tauri::State<'_, AiState>) -> Result<AiSettings, String> {
    #[cfg(mobile)]
    {
        let _ = (&app, &state);
        Err("Copying files with an AI tool isn't available on a phone or tablet.".into())
    }
    #[cfg(desktop)]
    {
        let Some(picked) = rfd::AsyncFileDialog::new().set_title("Choose a folder the AI may copy files into").pick_folder().await else {
            return Ok(settings_view(&state));
        };
        let path = picked.path().to_path_buf();
        let folder = tokio::task::spawn_blocking(move || copy::allowable_folder(&path)).await.map_err(|e| e.to_string())??;
        if state.add_pull_folder(folder.clone())? {
            save_settings(&app, &state, state.enabled(), state.0.flash.load(Ordering::Relaxed))?;
            state.log_setting(format!("AI tools may copy files into {}", folder.display()));
        }
        Ok(settings_view(&state))
    }
}

/// Remove a folder from "Folders the AI may copy into".
#[tauri::command]
pub async fn ai_remove_pull_folder(app: tauri::AppHandle, state: tauri::State<'_, AiState>, path: String) -> Result<AiSettings, String> {
    if state.remove_pull_folder(std::path::Path::new(&path)) {
        save_settings(&app, &state, state.enabled(), state.0.flash.load(Ordering::Relaxed))?;
        state.log_setting(format!("AI tools may no longer copy files into {path}"));
    }
    Ok(settings_view(&state))
}

#[tauri::command]
pub async fn ai_state(state: tauri::State<'_, AiState>) -> Result<Snapshot, String> {
    Ok(state.snapshot())
}

/// Share a server's tab with an AI tab. `name`: the tab's name, which the
/// AI will call it by.
#[tauri::command]
pub async fn ai_share(state: tauri::State<'_, AiState>, ssh: tauri::State<'_, crate::ssh_manager::SshState>, tab_id: String, session_id: String, name: String) -> Result<(), String> {
    if session_id.contains("::") || !ssh.connections.lock().await.contains_key(&session_id) {
        return Err("That server's tab isn't connected.".into());
    }
    let user = ssh.fork.session_users.lock().await.get(&session_id).cloned();
    // A quick connect is server 0: no saved server, so no settings of its
    // own and no "Always allow" for it.
    let server_id = ssh.fork.session_servers.lock().await.get(&session_id).map(|&id| i64::from(id)).filter(|&id| id > 0);
    state.share_as(&tab_id, &session_id, &name, user, server_id)
}

/// Stop sharing a server: with one AI tab, or (no `tab_id`) with any.
#[tauri::command]
pub async fn ai_unshare(state: tauri::State<'_, AiState>, session_id: String, tab_id: Option<String>) -> Result<(), String> {
    state.unshare(&session_id, tab_id.as_deref(), "stopped sharing");
    Ok(())
}

/// Your answer on the AI's request for a server by name (ask_server): share
/// it, once the page has opened its tab and it's connected, or not. Where
/// several saved servers have the name, `server_id` is the one picked, and
/// only one the request found is taken.
#[tauri::command]
pub async fn ai_answer_server_ask(
    state: tauri::State<'_, AiState>,
    ssh: tauri::State<'_, crate::ssh_manager::SshState>,
    id: u64,
    share: bool,
    server_id: Option<i64>,
) -> Result<(), String> {
    ask_server::answer(&state, &ssh, id, share, server_id).await
}

/// The panel's Stop for an AI tab.
#[tauri::command]
pub async fn ai_stop(state: tauri::State<'_, AiState>, tab_id: String) -> Result<(), String> {
    state.stop_tab(&tab_id);
    Ok(())
}

/// Stop in the AI panel, on a running follow (#179): ended on the server,
/// and the AI's next read says you stopped it.
#[tauri::command]
pub async fn ai_stop_follow(state: tauri::State<'_, AiState>, id: u64) -> Result<(), String> {
    state.stop_follow_by_you(id)
}

#[derive(Serialize)]
pub struct DoneOutput {
    output: String,
    ai_output: String,
}

/// A finished request's whole output, and what the AI got.
#[tauri::command]
pub async fn ai_output(state: tauri::State<'_, AiState>, id: u64) -> Result<DoneOutput, String> {
    let s = state.lock();
    let d = s.history.iter().find(|d| d.view.id == id).ok_or("That request's output is no longer kept.")?;
    Ok(DoneOutput { output: d.output.to_string(), ai_output: d.ai_output.to_string() })
}

/// Allow or Deny, from the card. `note`, with Deny: what the AI is told,
/// so it can try another way. `password`, with Allow on a card as root
/// (#120): the root password you typed there, which goes only to sudo's or
/// dzdo's own prompt, and is kept for this AI tab and server only.
#[tauri::command]
pub async fn ai_decide(state: tauri::State<'_, AiState>, id: u64, allow: bool, note: Option<String>, password: Option<String>) -> Result<(), String> {
    if allow {
        let password = typed_password(password)?;
        state.answer(id, Answer::Allow { by: BY_YOU.into(), password })
    } else {
        state.deny(id, note)
    }
}

/// End "Allow everything here" for a server shared with an AI tab.
#[tauri::command]
pub async fn ai_end_allow_all(state: tauri::State<'_, AiState>, tab_id: String, session_id: String) -> Result<(), String> {
    let n = state.end_allow_all_in(&tab_id, &session_id);
    if n == 0 {
        return Err("Everything wasn't allowed there.".into());
    }
    Ok(())
}

/// "Allow everything here until this AI tab closes", from the card, with
/// the root password typed on it, if it's a card as root that asks for one.
/// Allow or deny a whole `run_on_servers` request (#158), on every server
/// it names.
#[tauri::command]
pub async fn ai_decide_group(state: tauri::State<'_, AiState>, group: u64, allow: bool, note: Option<String>) -> Result<(), String> {
    state.decide_group(group, allow, note)
}

/// "Allow everything on these servers until this AI tab closes", for a
/// `run_on_servers` request's servers.
#[tauri::command]
pub async fn ai_allow_session_group(state: tauri::State<'_, AiState>, group: u64) -> Result<(), String> {
    state.allow_session_group(group)
}

#[tauri::command]
pub async fn ai_allow_session(state: tauri::State<'_, AiState>, id: u64, password: Option<String>) -> Result<(), String> {
    let password = typed_password(password)?;
    state.allow_session(id, password)
}

// ---------------------------------------------------------------------------
// Approval levels and rules (trust.rs): changed only from here, by you
// ---------------------------------------------------------------------------

/// Run `f` on the open profile's vault.
fn with_vault<T>(db: &crate::DbState, f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>) -> Result<T, String> {
    let guard = db.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
    let conn = guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
    f(conn)
}

/// Each level's name for the audit log, by its stored number less one.
const LEVEL_NAMES: [&str; 4] =
    ["ask every time", "run my allowed commands, ask for the rest", "run anything, just log it", "run commands that only read, ask before changes"];

fn level_name(level: Option<u8>) -> &'static str {
    match level {
        Some(l @ 1..=4) => LEVEL_NAMES[l as usize - 1],
        _ => "follows its folder or the default",
    }
}

/// An allow rule with a wildcard for a command that runs other commands
/// (`timeout *`) is saved only with the extra click that confirms it.
fn check_runner(pattern: &str, confirmed: Option<bool>) -> Result<(), String> {
    match trust::runner_warning(pattern) {
        Some(warning) if confirmed != Some(true) => Err(format!("{warning} Confirm it to save it.")),
        _ => Ok(()),
    }
}

/// "Always allow this command here", from the card: `pattern` (the command,
/// or it with ` *` for any arguments) goes on the server's allow list, and
/// this request is allowed. A server still at "Ask every time" goes to "Run
/// my allowed commands", or the rule would do nothing; the card says so.
#[tauri::command]
pub async fn ai_allow_always(
    state: tauri::State<'_, AiState>,
    db: tauri::State<'_, crate::DbState>,
    id: u64,
    pattern: String,
    confirm_runner: Option<bool>,
) -> Result<(), String> {
    let p = state.pending_view(id)?;
    check_runner(&pattern, confirm_runner)?;
    let server_id = p.server_id.filter(|_| p.can_allow_always).ok_or("This command can't be allowed with a rule: it isn't one simple command on a saved server.")?;
    if trust::glob_as_rule(&pattern, &p.command) {
        return Err(trust::GLOB_AS_RULE.into());
    }
    let pattern = trust::check_rule("allow", &pattern)?;
    if !trust::allow_matches(&pattern, &p.command) {
        return Err("That rule doesn't match this command.".into());
    }
    let (raised, what) = with_vault(&db, |conn| {
        let pattern = trust::add_rule(conn, "server", server_id, "allow", &pattern)?;
        let raised = trust::policy_for(conn, Some(server_id)).level == trust::ASK_EVERY_TIME;
        if raised {
            trust::set_level(conn, "server", server_id, Some(trust::ALLOWED_COMMANDS))?;
        }
        Ok((raised, format!("{}: always allow `{pattern}`", trust::describe(conn, "server", server_id))))
    })?;
    crate::save_vault_internal(&db)?;
    state.log_setting(if raised { format!("{what}; level set to {}", LEVEL_NAMES[1]) } else { what });
    state.answer(id, Answer::Allow { by: BY_YOU.into(), password: None })
}

/// "Keep as it was" for a server whose policy a teammate's move loosened
/// (trust::hold_loosened): it stays at "Ask every time" until you change it.
#[tauri::command]
pub async fn ai_keep_policy(state: tauri::State<'_, AiState>, db: tauri::State<'_, crate::DbState>, server_id: i64) -> Result<(), String> {
    let what = with_vault(&db, |conn| {
        trust::keep_as_it_was(conn, server_id)?;
        Ok(format!("{}: kept as it was after a teammate's move ({})", trust::describe(conn, "server", server_id), LEVEL_NAMES[0]))
    })?;
    crate::save_vault_internal(&db)?;
    state.log_setting(what);
    state.end_allow_all(|x| x.server_id == Some(server_id), "kept as it was after a teammate's move");
    state.changed();
    Ok(())
}

/// Everything set, for Settings › AI tools.
#[tauri::command]
pub async fn ai_trust_view(db: tauri::State<'_, crate::DbState>) -> Result<trust::TrustView, String> {
    with_vault(&db, trust::view)
}

/// One server's or folder's settings (or the default's), for its editor.
#[tauri::command]
pub async fn ai_trust_scope(db: tauri::State<'_, crate::DbState>, scope: String, id: i64) -> Result<trust::ScopeView, String> {
    with_vault(&db, |conn| trust::scope_view(conn, &scope, id))
}

/// Set the level of a server, a folder or the default; None takes it off.
#[tauri::command]
pub async fn ai_trust_set_level(
    state: tauri::State<'_, AiState>,
    db: tauri::State<'_, crate::DbState>,
    scope: String,
    id: i64,
    level: Option<u8>,
) -> Result<(), String> {
    let shared = state.shared_server_ids();
    let (what, reached) = with_vault(&db, |conn| {
        trust::set_level(conn, &scope, id, level)?;
        let reached: Vec<Option<i64>> = shared.into_iter().filter(|sid| trust::reaches(conn, *sid, &scope, id)).collect();
        Ok((format!("{}: {}", trust::describe(conn, &scope, id), level_name(level)), reached))
    })?;
    crate::save_vault_internal(&db)?;
    state.log_setting(what);
    // A level you set now holds, not "Allow everything here" from before.
    state.end_allow_all(|x| reached.contains(&x.server_id), "its approval level was changed");
    state.changed();
    Ok(())
}

/// Add an allow (`kind` "allow") or always-ask ("ask") rule.
#[tauri::command]
pub async fn ai_trust_add_rule(
    state: tauri::State<'_, AiState>,
    db: tauri::State<'_, crate::DbState>,
    scope: String,
    id: i64,
    kind: String,
    pattern: String,
    confirm_runner: Option<bool>,
) -> Result<String, String> {
    if kind == "allow" {
        check_runner(&pattern, confirm_runner)?;
    }
    let (pattern, what) = with_vault(&db, |conn| {
        let pattern = trust::add_rule(conn, &scope, id, &kind, &pattern)?;
        let what = format!("{}: {} `{pattern}`", trust::describe(conn, &scope, id), if kind == "allow" { "always allow" } else { "always ask" });
        Ok((pattern, what))
    })?;
    crate::save_vault_internal(&db)?;
    state.log_setting(what);
    state.changed();
    Ok(pattern)
}

#[tauri::command]
pub async fn ai_trust_remove_rule(state: tauri::State<'_, AiState>, db: tauri::State<'_, crate::DbState>, id: i64) -> Result<(), String> {
    let what = with_vault(&db, |conn| {
        Ok(trust::remove_rule(conn, id)?.map(|r| {
            let list = if r.kind == "allow" { "always allow" } else { "always ask" };
            format!("{}: removed {list} `{}`", trust::describe(conn, &r.scope, r.scope_id), r.pattern)
        }))
    })?;
    crate::save_vault_internal(&db)?;
    if let Some(what) = what {
        state.log_setting(what);
    }
    state.changed();
    Ok(())
}

// ---------------------------------------------------------------------------
// Home's menu
// ---------------------------------------------------------------------------

/// The AI tools Home's menu can start, while AI tools are on: those
/// installed.
pub(crate) fn home_tools(state: &AiState) -> Vec<(String, String, PathBuf)> {
    if !state.enabled() {
        return Vec::new();
    }
    tools::TOOLS.iter().filter_map(|t| Some((t.id.to_string(), t.label.to_string(), t.find()?))).collect()
}

/// The program an AI tool runs as `submarine --mcp`: this one, or on Linux
/// the AppImage it came in (its own folder is a mount that needs the
/// AppImage's start-up to find its libraries).
fn helper_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_absolute() && p.is_file()) {
        return Ok(image);
    }
    std::env::current_exe().map_err(|e| format!("Couldn't find Submarine itself: {e}"))
}

/// What Home starts for an AI tool, and the tab's label.
pub(crate) struct HomeLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Taken out of its environment: CREDENTIAL_ENV, less its own login.
    pub env_remove: Vec<&'static str>,
    /// Where it starts: its own folder (workspace), not your home folder.
    pub cwd: PathBuf,
    pub label: String,
}

/// The folder AI tools start in: `ai-workspace` in Submarine's data folder,
/// made if it isn't there, and on macOS and Linux only yours (0700). Not
/// your home folder, where Claude Code would read your files, ~/.ssh and
/// ~/.aws included, without asking: it asks before reading outside the
/// folder it started in.
fn workspace(data_dir: &std::path::Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("ai-workspace");
    let fail = |e: std::io::Error| format!("Couldn't make the AI tools' folder {}: {e}", dir.display());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        match std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(fail(e)),
        }
        // A folder of yours, not a link to somewhere else; 0700 even if it
        // was made otherwise.
        let m = std::fs::symlink_metadata(&dir).map_err(fail)?;
        // SAFETY: geteuid can't fail.
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } {
            return Err(format!("{} isn't a folder of yours.", dir.display()));
        }
        if m.mode() & 0o777 != 0o700 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(fail)?;
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(&dir).map_err(fail)?;
    Ok(dir)
}

/// What Home starts for the AI tool `id` in the terminal `tab_id`.
pub(crate) fn home_launch(app: &tauri::AppHandle, state: &AiState, id: &str, tab_id: &str) -> Result<HomeLaunch, String> {
    use tauri::Manager;
    let tool = tools::by_id(id).ok_or("Submarine doesn't know that AI tool.")?;
    let path = tool.find().ok_or_else(|| format!("{} isn't installed on this computer.", tool.label))?;
    let helper = helper_path()?;
    let data = app.path().app_data_dir().map_err(|e| format!("Couldn't find Submarine's data folder: {e}"))?;
    let cwd = workspace(&data)?;
    let launch = tool.launch(&path, &helper, Some(&data))?;
    let (label, mut env) = state.start_tab(tab_id, tool.short)?;
    env.extend(launch.env);
    Ok(HomeLaunch { program: launch.program, args: launch.args, env, env_remove: launch.env_remove, cwd, label })
}

/// A server's tab closed, or connects again (lib.rs, disconnect_session and
/// initiate_connection): no AI tab can reach it any more. A tab that
/// connects again is shared again by hand (#32, open question 11).
pub(crate) fn tab_gone(app: &tauri::AppHandle, session_id: &str, why: &str) {
    use tauri::Manager as _;
    if let Some(ai) = app.try_state::<AiState>() {
        ai.session_gone(session_id, why);
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("../lib.rs"),
            &[
                ("", "mod ai;", 1),
                ("initiate_connection", "ai::tab_gone(&app, &session_id, \"reconnected\")", 1),
                ("disconnect_session", "ai::tab_gone(&app, &session_id, \"tab closed\")", 1),
                ("teardown_profile", "ai.end_all_tabs()", 1),
            ],
        );
    }
}

#[cfg(test)]
mod tests;

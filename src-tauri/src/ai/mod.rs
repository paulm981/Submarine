//! AI help on open sessions, without sharing keys (#32). An AI tool started
//! from Home's menu reaches Submarine over a local channel (channel.rs,
//! through `submarine --mcp`, helper.rs) and asks it to run commands on the
//! servers you've shared with that AI tab. Submarine shows each request in
//! the AI panel, runs it only once you click Allow, on its own SSH channel
//! (exec.rs), and gives back the output with known secrets hidden
//! (filter.rs). Every request and decision goes in the audit log (audit.rs).
//!
//! The AI never gets a key or a password, and never connects to a server.
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
//!   on the server too.

mod audit;
mod channel;
mod claude;
mod exec;
mod filter;
pub mod helper;
mod mcp;

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
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
/// What the log keeps at most of a request's command (as written in its
/// JSON), and of a server name: the AI may send anything there, up to MCP's
/// 1 MB, and may ask, withdraw and ask again without you allowing anything.
const MAX_LOGGED_COMMAND: usize = 4 * 1024;
const MAX_LOGGED_NAME: usize = 256;
/// And of a note (why it was refused, an error), which may quote either.
const MAX_LOGGED_NOTE: usize = 2 * 1024;
/// Finished requests the panel keeps.
const HISTORY: usize = 20;
/// How long a second connection for an AI tab waits for the first to end
/// before it's refused: Claude Code reconnecting (`/mcp`) ends its old
/// helper and starts a new one, which may connect a moment before the app
/// has seen the old connection close.
const RECONNECT_GRACE: Duration = Duration::from_secs(5);
/// The panel's event.
const CHANGED: &str = "ai-changed";

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    #[serde(default)]
    enabled: bool,
}

type ExecFuture = Pin<Box<dyn Future<Output = Result<exec::Exec, String>> + Send>>;
/// Runs a command on a session's connection: the real one over SshState, or
/// a test server's. `(session_id, command, timeout, stop)`; see exec::run.
type Executor = Arc<dyn Fn(String, String, Duration, watch::Receiver<bool>) -> ExecFuture + Send + Sync>;

/// A server shared with an AI tab.
#[derive(Serialize, Clone, Debug)]
struct SharedServer {
    session_id: String,
    /// What the AI calls it: the tab's name, made unique for this AI tab.
    name: String,
    /// Who its tab logged in as, which commands run as; None if not known.
    user: Option<String>,
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
    /// It runs as root: the login user is root, or the command asks for
    /// root (asks_for_root).
    root: bool,
    timeout_secs: u64,
    asked_at: String,
}

struct Pending {
    view: PendingView,
    decide: oneshot::Sender<bool>,
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
}

struct Running {
    view: RunningView,
    tab_id: String,
    session_id: String,
    /// Stops it, on the server too (exec::run).
    stop: watch::Sender<bool>,
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
    /// denied, cancelled, done, failed, stopped.
    outcome: &'static str,
    exit_code: Option<i64>,
    output_bytes: usize,
    hidden: usize,
    cut: bool,
    timed_out: bool,
    error: Option<String>,
    at: String,
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
    running: Vec<RunningView>,
    history: Vec<DoneView>,
    audit_log: Option<String>,
}

#[derive(Default)]
struct Shared {
    enabled: bool,
    tabs: Vec<Tab>,
    pending: Vec<Pending>,
    running: Vec<Running>,
    history: VecDeque<Done>,
    next_id: u64,
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
    changed: Box<dyn Fn() + Send + Sync>,
    audit: audit::AuditLog,
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
/// A warning for you on the request's card, NOT a gate: it's easy to dodge
/// (`s\udo`, `$(echo sudo)`, `runuser`, a script that calls sudo), so
/// nothing may be allowed or refused because of it. Anything that has to
/// keep root out (step 3 of #32: running as another account) must decide on
/// its own `as_root` and treat "can't tell" as root.
pub(crate) fn asks_for_root(command: &str) -> bool {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"(?:^|[\s;|&(){}`'"/!])(?:sudo|dzdo|su|doas|pkexec)(?:$|[\s;|&(){}`'"])"#).unwrap());
    RE.is_match(command)
}

/// A request runs as root: its server's tab logged in as root, or its
/// command asks for root. A warning, as asks_for_root is.
fn runs_as_root(user: Option<&str>, command: &str) -> bool {
    user == Some("root") || asks_for_root(command)
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
    fn new(exec: Executor, changed: Box<dyn Fn() + Send + Sync>, audit_dir: Option<PathBuf>) -> AiState {
        AiState(Arc::new(Inner {
            shared: Mutex::new(Shared::default()),
            listener: Mutex::new(None),
            exec,
            changed,
            audit: audit::AuditLog::new(audit_dir),
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
        let emit_app = app.clone();
        let changed = Box::new(move || {
            let _ = emit_app.emit(CHANGED, ());
        });
        let state = AiState::new(exec, changed, app.path().app_log_dir().ok());
        let settings: Settings = crate::local_open::read_setting(app, SETTINGS_FILE).unwrap_or_default();
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
        self.share_as(tab_id, session_id, name, None)
    }

    /// Share `session_id`, whose tab logged in as `user`, with `tab_id`.
    fn share_as(&self, tab_id: &str, session_id: &str, name: &str, user: Option<String>) -> Result<(), String> {
        let mut s = self.lock();
        let tab = s.tabs.iter_mut().find(|t| t.id == tab_id).ok_or("That AI tab isn't open any more.")?;
        if tab.servers.iter().any(|x| x.session_id == session_id) {
            return Ok(());
        }
        let name = unique_name(&tab.servers, name);
        tab.servers.push(SharedServer { session_id: session_id.to_string(), name: name.clone(), user });
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
        // Dropping a request's sender is what cancels it.
        s.pending.retain(|p| !which(&p.view.tab_id, &p.view.session_id));
        s.running.retain(|r| {
            let stop = which(&r.tab_id, &r.session_id);
            if stop {
                let _ = r.stop.send(true);
            }
            !stop
        });
    }

    /// Your Allow or Deny. Allowed, the request moves from waiting to
    /// running in the same step, with its stop signal, and only while its
    /// AI tab still has the server shared: so unsharing, Stop or a
    /// reconnect at the same moment either comes first (and it never runs)
    /// or finds it running (and stops it).
    fn decide(&self, id: u64, allow: bool) -> Result<(), String> {
        let result = {
            let mut s = self.lock();
            let at = s.pending.iter().position(|p| p.view.id == id).ok_or("That request isn't waiting any more.")?;
            let p = s.pending.remove(at);
            let still_shared = s.tabs.iter().any(|t| t.id == p.view.tab_id && t.servers.iter().any(|x| x.session_id == p.view.session_id));
            if !allow {
                let _ = p.decide.send(false);
                Ok(())
            } else if !still_shared {
                // Dropped: cancelled, as access has ended.
                Err("That server isn't shared with the AI any more.".to_string())
            } else {
                let v = &p.view;
                s.running.push(Running {
                    view: RunningView {
                        id: v.id,
                        tab_label: v.tab_label.clone(),
                        session_id: v.session_id.clone(),
                        server: v.server.clone(),
                        command: v.command.clone(),
                        started_at: now(),
                    },
                    tab_id: v.tab_id.clone(),
                    session_id: v.session_id.clone(),
                    stop: p.stop.clone(),
                });
                if p.decide.send(true).is_err() {
                    // Nobody is waiting for the answer any more.
                    s.running.pop();
                }
                Ok(())
            }
        };
        self.changed();
        result
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        Snapshot {
            enabled: s.enabled,
            tabs: s
                .tabs
                .iter()
                .map(|t| TabView { id: t.id.clone(), label: t.label.clone(), servers: t.servers.clone(), intruder_at: t.intruder_at.clone() })
                .collect(),
            pending: s.pending.iter().map(|p| p.view.clone()).collect(),
            running: s.running.iter().map(|r| r.view.clone()).collect(),
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
            "list_shared" => self.list_shared(tab_id),
            "run_command" => {
                let server = args.get("server").and_then(|v| v.as_str()).unwrap_or("");
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let timeout = args.get("timeout_seconds").and_then(|v| v.as_u64());
                self.run_command(tab_id, server, command, timeout, cancel).await
            }
            _ => ToolResult::err(format!("Submarine has no tool called {name}.")),
        }
    }

    fn list_shared(&self, tab_id: &str) -> ToolResult {
        let s = self.lock();
        let Some(tab) = s.tabs.iter().find(|t| t.id == tab_id) else {
            return ToolResult::err("This AI tab's access has ended.");
        };
        if tab.servers.is_empty() {
            return ToolResult::ok(
                "No servers are shared with you yet. Ask the user to open the server's tab in Submarine and click Share with AI on its toolbar.",
            );
        }
        let names: Vec<&str> = tab.servers.iter().map(|x| x.name.as_str()).collect();
        ToolResult::ok(serde_json::json!({ "servers": names }).to_string())
    }

    /// Refused before it reaches you: logged, and the AI told why.
    fn refuse(&self, label: &str, server: &str, command: &str, why: String) -> ToolResult {
        let mut e = audit::Entry::new("request", label);
        e.request = Some("run_command");
        e.server = Some(for_log(server, MAX_LOGGED_NAME)).filter(|s| !s.is_empty());
        e.command = Some(for_log(command, MAX_LOGGED_COMMAND));
        e.decision = Some("refused");
        e.by = Some("Submarine");
        e.note = Some(why.clone());
        self.audit(e);
        ToolResult::err(why)
    }

    async fn run_command(&self, tab_id: &str, server: &str, command: &str, timeout: Option<u64>, mut cancel: watch::Receiver<bool>) -> ToolResult {
        // Checked before anything is shown to you.
        let (label, session_id, server, user, id, epoch, mut rx, stop_tx, stop_rx) = {
            let mut s = self.lock();
            let next_id = s.next_id + 1;
            let epoch = s.epoch;
            let waiting = s.pending.iter().filter(|p| p.view.tab_id == tab_id).count();
            let Some(tab) = s.tabs.iter_mut().find(|t| t.id == tab_id) else {
                return ToolResult::err("This AI tab's access has ended.");
            };
            let label = tab.label.clone();
            // Every call counts toward the rate limit, refused or not; past
            // it, one line in the log a window, not one a call.
            if let Some(cutoff) = Instant::now().checked_sub(RATE_WINDOW) {
                while tab.asked.front().is_some_and(|t| *t < cutoff) {
                    tab.asked.pop_front();
                }
            }
            if tab.asked.len() >= RATE_MAX {
                let log = tab.rate_logged.is_none_or(|t| t.elapsed() >= RATE_WINDOW);
                if log {
                    tab.rate_logged = Some(Instant::now());
                }
                drop(s);
                let why = format!("Too many requests: at most {RATE_MAX} a minute. Wait a moment.");
                if !log {
                    return ToolResult::err(why);
                }
                return self.refuse(&label, server, command, why);
            }
            tab.asked.push_back(Instant::now());
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
                let found = found.clone();
                if waiting >= MAX_WAITING {
                    return Err(refuse(format!("{MAX_WAITING} requests are already waiting for the user. Wait for them first.")));
                }
                Ok(found)
            })();
            let found = match checked {
                Ok(f) => f,
                Err((label, why)) => {
                    drop(s);
                    return self.refuse(&label, server, command, why);
                }
            };
            s.next_id = next_id;
            let (tx, rx) = oneshot::channel();
            let (stop_tx, stop_rx) = watch::channel(false);
            s.pending.push(Pending {
                view: PendingView {
                    id: next_id,
                    tab_id: tab_id.to_string(),
                    tab_label: label.clone(),
                    session_id: found.session_id.clone(),
                    server: found.name.clone(),
                    user: found.user.clone(),
                    command: command.to_string(),
                    root: runs_as_root(found.user.as_deref(), command),
                    timeout_secs: timeout.unwrap_or(DEFAULT_TIMEOUT),
                    asked_at: now(),
                },
                decide: tx,
                stop: stop_tx.clone(),
            });
            (label, found.session_id, found.name, found.user, next_id, epoch, rx, stop_tx, stop_rx)
        };
        self.changed();
        let root = runs_as_root(user.as_deref(), command);
        let limit = Duration::from_secs(timeout.unwrap_or(DEFAULT_TIMEOUT));
        let entry_for = |event: &'static str| {
            let mut e = audit::Entry::new(event, &label);
            e.id = Some(id);
            e.request = Some("run_command");
            e.server = Some(server.clone());
            e.user = user.clone();
            e.command = Some(for_log(command, MAX_LOGGED_COMMAND));
            e.root = root;
            e
        };
        let entry = || entry_for("request");
        let done = |outcome: &'static str| Done {
            view: DoneView {
                id,
                tab_label: label.clone(),
                server: server.clone(),
                command: command.to_string(),
                root,
                outcome,
                exit_code: None,
                output_bytes: 0,
                hidden: 0,
                cut: false,
                timed_out: false,
                error: None,
                at: now(),
            },
            output: Zeroizing::default(),
            ai_output: Zeroizing::default(),
        };

        // Waiting for you. The answer: Some(allowed), or None when access
        // ended first (the request dropped by cancel_where or decide).
        let decision: Result<Option<bool>, ()> = tokio::select! {
            biased;
            d = &mut rx => Ok(d.ok()),
            _ = withdrawal(&mut cancel) => Err(()),
        };
        let decision = match decision {
            Ok(d) => d,
            // The AI withdrew it: unless you or an end of access got to it
            // first, it's taken off the waiting list here.
            Err(()) => {
                let was_waiting = {
                    let mut s = self.lock();
                    let before = s.pending.len();
                    s.pending.retain(|p| p.view.id != id);
                    before != s.pending.len()
                };
                if was_waiting {
                    let mut e = entry();
                    e.decision = Some("cancelled");
                    e.note = Some("cancelled by the AI tool".into());
                    self.audit(e);
                    self.remember(epoch, done("cancelled"));
                    self.changed();
                    return ToolResult::err("Cancelled before the user decided: the request was withdrawn.");
                }
                // Taken off under the same lock as the answer was given, so
                // it's there now (or the request was dropped).
                rx.try_recv().ok()
            }
        };
        let Some(allowed) = decision else {
            let mut e = entry();
            e.decision = Some("cancelled");
            e.note = Some("access ended".into());
            self.audit(e);
            self.remember(epoch, done("cancelled"));
            self.changed();
            return ToolResult::err("Cancelled before the user decided: the server isn't shared any more.");
        };
        if !allowed {
            let mut e = entry();
            e.decision = Some("denied");
            e.by = Some("you");
            self.audit(e);
            self.remember(epoch, done("denied"));
            self.changed();
            return ToolResult::err("Denied by the user.");
        }

        // Logged now, as you allowed it: a long command, or one that hangs,
        // is in the log while it runs, not only once it ends. Its "finished"
        // line follows, with the same id.
        let mut e = entry();
        e.decision = Some("allowed");
        e.by = Some("you");
        self.audit(e);

        // Allowed: decide() has put it in the running list with its stop
        // signal, which an end of access sets. The AI withdrawing it sets it
        // too. Either way exec::run ends it on the server before returning;
        // it runs as a task of its own, so it does even if this is dropped.
        let withdrawn = |c: &watch::Receiver<bool>| *c.borrow();
        if withdrawn(&cancel) {
            let _ = stop_tx.send(true);
        }
        let mut task = tokio::spawn((self.0.exec)(session_id.clone(), command.to_string(), limit, stop_rx));
        let joined = tokio::select! {
            r = &mut task => r,
            _ = withdrawal(&mut cancel) => {
                let _ = stop_tx.send(true);
                task.await
            }
        };
        let result: Result<exec::Exec, String> = joined.unwrap_or_else(|e| Err(format!("It stopped unexpectedly: {e}")));
        // Still listed: nothing ended its access while it ran.
        let still_listed = {
            let mut s = self.lock();
            let before = s.running.len();
            s.running.retain(|r| r.view.id != id);
            before != s.running.len()
        };
        let stopped_by_ai = still_listed && withdrawn(&cancel);

        let mut e = entry_for("finished");
        let mut d = done("done");
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
                let answer = format!("{head}--- output ---\n{}", ai.as_str());
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
                ToolResult::ok(answer)
            }
            Err(err) => {
                e.note = Some(err.clone());
                d.view.outcome = "failed";
                d.view.error = Some(err.clone());
                ToolResult::err(format!("Couldn't run it: {err}"))
            }
        };
        self.audit(e);
        self.remember(epoch, d);
        self.changed();
        answer
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AiSettings {
    enabled: bool,
    /// Where Claude Code is, when it's installed.
    claude: Option<String>,
    available: bool,
}

fn settings_view(state: &AiState) -> AiSettings {
    AiSettings {
        enabled: state.enabled(),
        claude: claude::find().map(|p| p.to_string_lossy().into_owned()),
        available: cfg!(not(target_os = "android")),
    }
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
    crate::local_open::write_setting(&app, SETTINGS_FILE, &Settings { enabled }, "the AI tools setting")?;
    if enabled {
        state.turn_on()?;
    } else {
        state.turn_off().await;
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
    let user = ssh.session_users.lock().await.get(&session_id).cloned();
    state.share_as(&tab_id, &session_id, &name, user)
}

/// Stop sharing a server: with one AI tab, or (no `tab_id`) with any.
#[tauri::command]
pub async fn ai_unshare(state: tauri::State<'_, AiState>, session_id: String, tab_id: Option<String>) -> Result<(), String> {
    state.unshare(&session_id, tab_id.as_deref(), "stopped sharing");
    Ok(())
}

/// The panel's Stop for an AI tab.
#[tauri::command]
pub async fn ai_stop(state: tauri::State<'_, AiState>, tab_id: String) -> Result<(), String> {
    state.stop_tab(&tab_id);
    Ok(())
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

#[tauri::command]
pub async fn ai_decide(state: tauri::State<'_, AiState>, id: u64, allow: bool) -> Result<(), String> {
    state.decide(id, allow)
}

// ---------------------------------------------------------------------------
// Home's menu
// ---------------------------------------------------------------------------

/// An AI tool Home's menu can start, while AI tools are on and it's installed.
pub(crate) fn home_tools(state: &AiState) -> Vec<(String, String, PathBuf)> {
    if !state.enabled() {
        return Vec::new();
    }
    claude::find().map(|p| (CLAUDE_ID.to_string(), claude::LABEL.to_string(), p)).into_iter().collect()
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
    if id != CLAUDE_ID {
        return Err("Submarine doesn't know that AI tool.".into());
    }
    let path = claude::find().ok_or("Claude Code isn't installed on this computer.")?;
    let helper = helper_path()?;
    let data = app.path().app_data_dir().map_err(|e| format!("Couldn't find Submarine's data folder: {e}"))?;
    let cwd = workspace(&data)?;
    let launch = claude::launch(&path, &helper, Some(&data))?;
    let (label, mut env) = state.start_tab(tab_id, claude::SHORT)?;
    env.extend(launch.env);
    Ok(HomeLaunch { program: launch.program, args: launch.args, env, env_remove: launch.env_remove, cwd, label })
}

#[cfg(test)]
mod tests;

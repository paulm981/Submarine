use russh::client;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Host-key fingerprint in the format every `known_hosts` row was written
/// with: un-padded base64 of SHA-256 over the key blob, WITHOUT the
/// `SHA256:` prefix. russh-keys 0.40 produced exactly that; ssh-key's
/// `Fingerprint` Display adds the prefix, so we strip it to keep existing
/// rows matching (a format change would fire a false "KEY CHANGED" warning
/// for every pinned host).
pub fn host_key_fingerprint(key: &PublicKey) -> String {
    let fp = key.fingerprint(HashAlg::Sha256).to_string();
    fp.strip_prefix("SHA256:").map(str::to_owned).unwrap_or(fp)
}

/// Collapse an SSH host-key algorithm name to its key FAMILY — the unit both
/// the "same key type, different fingerprint ⇒ key CHANGED" decision and the
/// replace-on-accept DELETE (`save_approved_host_key`) work on. Every
/// `known_hosts.key_type` comparison goes through this.
///
/// russh-keys 0.40 recorded an RSA host key under the NEGOTIATED signature
/// algorithm (`rsa-sha2-512`, `rsa-sha2-256` or `ssh-rsa`), and that is what
/// older rows contain; russh 0.63 reports the key's own type (`ssh-rsa`). All
/// RSA spellings are therefore one family. ECDSA stays per-curve (a P-256 and
/// a P-384 key are different host keys, as in OpenSSH) and Ed25519 is its own
/// family. Case and surrounding whitespace are ignored, and a
/// `-cert-v01@openssh.com` suffix maps a certificate type to the family of the
/// key it certifies.
pub(crate) fn key_type_family(key_type: &str) -> String {
    let kt = key_type.trim().to_ascii_lowercase();
    let base = kt.strip_suffix("-cert-v01@openssh.com").unwrap_or(&kt);
    match base {
        "ssh-rsa" | "rsa-sha2-256" | "rsa-sha2-512" => "ssh-rsa".to_string(),
        other => other.to_string(),
    }
}

/// The kinds of host key (`key_type_family`) on file for host:port, so a
/// connection can ask the server for those first (`prefer_known_host_keys`).
pub(crate) fn known_host_families(conn: &rusqlite::Connection, host: &str, port: u16) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare("SELECT key_type FROM known_hosts WHERE host=?1 AND port=?2 AND key_type IS NOT NULL") else {
        return Vec::new();
    };
    let mut families: Vec<String> = stmt
        .query_map(rusqlite::params![host, port], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(Result::ok).map(|kt| key_type_family(&kt)).collect())
        .unwrap_or_default();
    families.sort();
    families.dedup();
    families
}

/// What the keys on file for a host say about the one it offers.
#[derive(Debug, PartialEq)]
pub(crate) enum HostKeyVerdict {
    /// On file: trusted.
    Known,
    /// Keys are on file for this host, and this isn't one of them: the
    /// "changed" warning, with what was trusted before (each with its kind).
    Changed { prior: Vec<String> },
    /// Nothing on file: the first-time prompt.
    New,
}

/// Any key on file for host:port that isn't the offered one makes it a change,
/// whatever its kind. A connection asks for the kinds on file first
/// (`prefer_known_host_keys`), so a server that still has its known key shows
/// it; one that shows another kind instead has replaced it, or isn't the same
/// server. Only the same kind used to count, so a man-in-the-middle without
/// the pinned Ed25519 key got the ordinary first-time prompt by offering an
/// ECDSA or RSA key.
pub(crate) fn judge_host_key(
    conn: &rusqlite::Connection,
    host: &str,
    port: u16,
    fingerprint: &str,
) -> rusqlite::Result<HostKeyVerdict> {
    let mut stmt = conn.prepare("SELECT fingerprint, key_type FROM known_hosts WHERE host=?1 AND port=?2")?;
    let rows = stmt.query_map(rusqlite::params![host, port], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut prior = Vec::new();
    for row in rows {
        let (saved_fp, saved_kt) = row?;
        if saved_fp == fingerprint {
            return Ok(HostKeyVerdict::Known);
        }
        prior.push(match saved_kt {
            Some(kt) => format!("{saved_fp} ({kt})"),
            None => saved_fp,
        });
    }
    Ok(if prior.is_empty() { HostKeyVerdict::New } else { HostKeyVerdict::Changed { prior } })
}

/// Persist a host key the user just approved in the fingerprint prompt.
///
/// When the prompt was a KEY-CHANGED warning (`replace`), every key on file
/// for this host:port is deleted first, of whatever kind: the user has just
/// said this is the server's identity now. Otherwise the next connection
/// would find a RETIRED fingerprint still trusted ("any row matches =
/// trusted"), defeating the warning, and a server whose old key leaked could
/// be impersonated with it.
///
/// All-or-nothing: manual BEGIN/COMMIT (we only hold `&Connection` through the
/// mutex guard, not the `&mut Connection` rusqlite's `transaction()` needs),
/// rolled back on any failure so the host is never left with zero recorded
/// fingerprints — which would silently downgrade the next connection from
/// "mismatch" to "first time".
fn save_approved_host_key(
    conn: &rusqlite::Connection,
    host: &str,
    port: u16,
    fingerprint: &str,
    key_type: &str,
    replace: bool,
) -> rusqlite::Result<()> {
    let result: rusqlite::Result<()> = (|| {
        conn.execute("BEGIN", [])?;
        if replace {
            conn.execute("DELETE FROM known_hosts WHERE host=?1 AND port=?2", rusqlite::params![host, port])?;
        }
        conn.execute(
            "INSERT INTO known_hosts (host, port, fingerprint, key_type) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![host, port, fingerprint, key_type],
        )?;
        conn.execute("COMMIT", [])?;
        Ok(())
    })();
    if result.is_err() {
        let _ = conn.execute("ROLLBACK", []);
    }
    result
}

/// Public-key auth with the `bool` contract callers relied on under russh
/// 0.40. Picks the RSA hash the server advertises (no-op for non-RSA keys).
pub async fn authenticate_with_key<H: client::Handler>(
    session: &mut client::Handle<H>,
    user: &str,
    key: PrivateKey,
) -> Result<bool, russh::Error> {
    Ok(authenticate_with_key_result(session, user, key).await?.success())
}

/// [`authenticate_with_key`] with the server's full answer — the methods it
/// still offers and whether the key was a partial success — so the caller can
/// decide what to try next.
pub async fn authenticate_with_key_result<H: client::Handler>(
    session: &mut client::Handle<H>,
    user: &str,
    key: PrivateKey,
) -> Result<client::AuthResult, russh::Error> {
    let hash_alg = if key.algorithm().is_rsa() {
        session.best_supported_rsa_hash().await?.flatten()
    } else {
        None
    };
    let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
    session.authenticate_publickey(user, key).await
}

/// Certificate login with a private key held here (a key file). An RSA
/// certificate is signed with the hash the server asks for, as a plain RSA
/// key is: russh's `authenticate_openssh_cert` always names it
/// `ssh-rsa-cert-v01@openssh.com` (SHA-1), which OpenSSH 8.8 and later refuse.
pub async fn authenticate_with_certificate<H: client::Handler>(
    session: &mut client::Handle<H>,
    user: &str,
    key: Arc<PrivateKey>,
    cert: russh::keys::Certificate,
) -> Result<client::AuthResult, russh::Error> {
    let hash_alg = if key.algorithm().is_rsa() {
        session.best_supported_rsa_hash().await?.flatten()
    } else {
        None
    };
    session
        .authenticate_certificate_with(user, cert, hash_alg, &mut KeyFileSigner(key))
        .await
        .map_err(|e| match e {
            russh::AgentAuthError::Send(_) => russh::Error::SendError,
            russh::AgentAuthError::Key(e) => russh::Error::Keys(e),
        })
}

/// Signs a login the way an ssh-agent answers one, with a key held here: the
/// signature is appended to the data as an SSH string.
struct KeyFileSigner(Arc<PrivateKey>);

impl russh::Signer for KeyFileSigner {
    type Error = russh::AgentAuthError;

    fn auth_sign(
        &mut self,
        _identity: &russh::keys::agent::AgentIdentity,
        hash_alg: Option<HashAlg>,
        mut to_sign: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        let signed = sign_appended(&self.0, hash_alg, &mut to_sign).map(|()| to_sign);
        std::future::ready(signed.map_err(russh::AgentAuthError::Key))
    }
}

fn sign_appended(key: &PrivateKey, hash_alg: Option<HashAlg>, data: &mut Vec<u8>) -> Result<(), russh::keys::Error> {
    use russh::keys::signature::Signer;
    use russh::keys::ssh_encoding::Encode;
    use russh::keys::ssh_key::private::KeypairData;
    let signature = match key.key_data() {
        KeypairData::Rsa(rsa) => Signer::try_sign(&(rsa, hash_alg), data.as_slice()),
        other => Signer::try_sign(other, data.as_slice()),
    }
    .map_err(|_| russh::keys::ssh_key::Error::Crypto)?;
    signature.encode_vec()?.encode(data)?;
    Ok(())
}

/// Password auth with the `bool` contract callers relied on under russh 0.40.
pub async fn authenticate_with_password<H: client::Handler>(
    session: &mut client::Handle<H>,
    user: &str,
    password: &str,
) -> Result<bool, russh::Error> {
    Ok(session.authenticate_password(user, password).await?.success())
}

/// Per-terminal command. We deliberately split data from resize at the
/// channel level: keystrokes flow through `Data` on an mpsc, while resizes
/// land in a tokio::sync::watch (last-wins, coalesces a 60Hz drag burst
/// into one effective resize). Keeping them on the same FIFO mpsc meant
/// typed bytes could queue behind dozens of resize events during a
/// window drag — visible as keystrokes arriving seconds late.
pub enum TerminalCommand {
    Data(Vec<u8>),
}

#[derive(Clone, Copy, Debug)]
pub struct PtySize {
    pub cols: u32,
    pub rows: u32,
}

/// Drive an interactive PTY channel — an SSH shell (`open_terminal`) or a
/// `docker exec -it` (`docker::open_container_terminal`) — for the life of the
/// tab: output → coalesced batches into `output` (term_output.rs: the page's
/// channel, paced by what it has drawn), keystrokes / resizes → the channel,
/// then, once the last output is out, `terminal-closed-{id}`. See `pty_pump`.
pub async fn run_pty_pump(
    app: AppHandle,
    terminal_id: String,
    channel: russh::Channel<client::Msg>,
    rx: mpsc::Receiver<TerminalCommand>,
    resize_rx: tokio::sync::watch::Receiver<PtySize>,
    output: crate::term_output::OutputSink,
    // Output already read from the channel before the pump took it (docker's
    // root prompt check): it goes first, through the same filter.
    early: Vec<u8>,
) {
    pty_pump(channel, rx, resize_rx, early, |buf| output.push(buf)).await;
    output.finish(move || {
        let _ = app.emit(&format!("terminal-closed-{}", terminal_id), serde_json::json!({}));
    });
}

/// The pump behind `run_pty_pump`, with the output sink abstracted (`flush`
/// gets each coalesced batch and must leave the buffer empty, as
/// `OutputSink::push` does) so tests can run it against a real connection.
///
/// The channel is SPLIT so its read side is drained continuously, whatever the
/// write side is doing. Since russh 0.50 every channel has a bounded queue and
/// the connection's single protocol task *awaits* room in it — so a reader
/// that stops to await a write stalls the ENTIRE connection (every terminal,
/// tunnel and SFTP op on it). The old single-`select!` loop did exactly that:
/// awaiting the write of a large paste (which waits for the server's window)
/// while the server floods output filled this channel's queue, blocked the
/// protocol task, and so never delivered the window adjust the write was
/// waiting on — a deadlock. Here the writer runs in its own task; the reader
/// only ever waits on the channel, the flush timer and the writer's stop
/// signal.
async fn pty_pump<F>(
    channel: russh::Channel<client::Msg>,
    mut rx: mpsc::Receiver<TerminalCommand>,
    mut resize_rx: tokio::sync::watch::Receiver<PtySize>,
    early: Vec<u8>,
    mut flush: F,
) where
    F: FnMut(&mut Vec<u8>),
{
    use russh::ChannelMsg;

    let (mut read_half, write_half) = channel.split();

    // Writer: keystrokes + last-wins resizes. It signals `stop` in the same
    // cases the old loop `break`ed on the write side: the UI dropped the
    // terminal (all senders gone → close the channel), the resize watch was
    // dropped, or the transport rejected a write.
    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let writer = tokio::spawn(async move {
        loop {
            tokio::select! {
                opt_cmd = rx.recv() => match opt_cmd {
                    Some(TerminalCommand::Data(data)) => {
                        if write_half.data_bytes(data).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = write_half.close().await;
                        break;
                    }
                },
                // `changed().await` resolves on every Sender::send(). We then
                // read the LATEST value with .borrow() so coalesced bursts
                // collapse to one window_change call.
                changed = resize_rx.changed() => {
                    if changed.is_err() {
                        break; // all senders dropped
                    }
                    let size = *resize_rx.borrow();
                    let _ = write_half.window_change(size.cols, size.rows, 0, 0).await;
                }
            }
        }
        let _ = stop_tx.send(());
    });

    // Coalesce PTY output: accumulate channel bytes and flush at most every
    // ~8ms, or sooner once a burst passes FLUSH_CAP, instead of one message
    // per SSH packet. 8ms (measured from the FIRST buffered byte) is
    // imperceptible for interactive echo. The sink then paces what reaches
    // the page (term_output.rs).
    const FLUSH_CAP: usize = 256 * 1024;
    const FLUSH_WINDOW: std::time::Duration = std::time::Duration::from_millis(8);
    let mut out_buf: Vec<u8> = Vec::new();
    // What the page shows is cut down first: a number past four digits in a
    // control sequence can freeze the page (term_filter.rs).
    let mut limiter = crate::term_filter::CsiLimiter::default();
    if !early.is_empty() {
        limiter.filter(&early, &mut out_buf);
        flush(&mut out_buf);
    }
    // A flush timer armed ONLY while bytes are buffered. When the buffer is
    // empty its deadline is parked far in the future, so an open-but-idle
    // terminal wakes this task zero times (a free-running interval would fire
    // ~125x/sec doing nothing). The buffer going empty -> non-empty re-arms
    // it to now + FLUSH_WINDOW; a flush parks it again.
    let park = || tokio::time::Instant::now() + std::time::Duration::from_secs(24 * 3600);
    let flush_timer = tokio::time::sleep_until(park());
    tokio::pin!(flush_timer);
    loop {
        tokio::select! {
            msg_opt = read_half.wait() => match msg_opt {
                Some(ChannelMsg::Data { ref data })
                | Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                    let was_empty = out_buf.is_empty();
                    limiter.filter(data, &mut out_buf);
                    if out_buf.len() >= FLUSH_CAP {
                        flush(&mut out_buf);
                    } else if was_empty {
                        flush_timer.as_mut().reset(tokio::time::Instant::now() + FLUSH_WINDOW);
                    }
                }
                // Flush whatever's buffered before the terminal goes away so
                // the last screenful isn't lost. `None` = channel gone (e.g.
                // after disconnect_session).
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                    flush(&mut out_buf);
                    break;
                }
                Some(_) => {}
            },
            _ = &mut flush_timer => {
                flush(&mut out_buf);
                // Park until the next buffered byte re-arms the timer.
                flush_timer.as_mut().reset(park());
            }
            // Writer finished (UI closed the tab, or the transport is dead):
            // flush the last buffered output — the terminal UI may still be
            // mounted — and stop.
            _ = &mut stop_rx => {
                flush(&mut out_buf);
                break;
            }
        }
    }
    // Reader done: stop the writer too. Dropping its `rx` makes later
    // write_terminal_data calls silent no-ops, as before.
    writer.abort();
}

/// A session's route through its jump hosts (`SshState::jump_connections`):
/// their Handles, the last jump host last, each shared so a check through it
/// (ping.rs) takes its own reference instead of holding the map; the
/// connections close once the last reference goes. `jump_id`: the saved
/// jump host it went through, None for a ProxyJump from ~/.ssh/config.
pub struct JumpRoute {
    pub jump_id: Option<i32>,
    pub handles: Vec<Arc<client::Handle<ClientHandler>>>,
}

pub struct SshState {
    pub fp_txs: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Pending keyboard-interactive (2FA / OTP) prompt responses, keyed by the
    /// same per-connect nonce as `fp_txs`. The value is `Some(answers)` when
    /// the user submits, or `None` when they cancel. A server can issue several
    /// sequential InfoRequests in one auth, so the connect worker re-inserts a
    /// fresh sender under the nonce for each round; `submit_kbi_response`
    /// removes-and-sends. Separate map from `fp_txs` because the two prompts
    /// never overlap in time (host-key check runs during the handshake,
    /// keyboard-interactive runs during auth) but carry different value types.
    pub kbi_txs: Arc<Mutex<HashMap<String, oneshot::Sender<Option<Vec<String>>>>>>,
    pub connections: Arc<Mutex<HashMap<String, Arc<Mutex<client::Handle<ClientHandler>>>>>>,
    /// ProxyJump bastion handles, keyed by the TARGET session_id. Each target
    /// session that routes through a jump host stows the jump's live `Handle`
    /// here purely to keep it (and thus the direct-tcpip channel carrying the
    /// target's transport) alive for the session's lifetime. Removed — and so
    /// dropped/closed — on reconnect teardown, disconnect, and profile close.
    pub jump_connections: Arc<Mutex<HashMap<String, JumpRoute>>>,
    pub terminal_txs: Arc<Mutex<HashMap<String, mpsc::Sender<TerminalCommand>>>>,
    /// Per-terminal "last requested PTY size" watch. The PTY task selects
    /// on this in parallel with `terminal_txs` and forwards `window_change`
    /// to the server. Using a watch (last-wins) means a 60Hz resize burst
    /// during a window drag collapses to a single SSH message instead of
    /// dozens, AND the keystroke FIFO can't be blocked behind resizes.
    pub resize_txs: Arc<Mutex<HashMap<String, tokio::sync::watch::Sender<PtySize>>>>,
    pub sftp_sessions: Arc<Mutex<HashMap<String, Arc<russh_sftp::client::SftpSession>>>>,
    /// Active SSH port-forwards keyed by tunnel id. See `crate::tunnel`.
    pub tunnels: Arc<Mutex<HashMap<String, crate::tunnel::ActiveTunnel>>>,
    /// For each connected session, the map of server ports we've asked the
    /// server to forward back to us. Populated by `tunnel::start_tunnel` for
    /// "R" tunnels and consulted by `ClientHandler` when a forwarded channel
    /// arrives.
    pub forwarded_targets: Arc<Mutex<HashMap<String, crate::tunnel::ForwardedTargets>>>,
    /// One AtomicBool per in-flight SFTP transfer, keyed by transfer id.
    /// `sftp_cancel_transfer` flips the flag; the chunked read/write loops
    /// in `sftp_download_file` / `sftp_upload_file` poll it each iteration
    /// and bail out with a cancelled-status event when it goes true.
    pub transfer_cancels: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    /// Per-session tunnel spec memory. Survives reconnect cycles so that
    /// `initiate_connection` can re-establish every forward the user had
    /// open — including ad-hoc ones not in the saved server row.
    pub session_tunnel_specs: Arc<Mutex<HashMap<String, Vec<crate::tunnel::TunnelSpec>>>>,
    /// Monotonic generation counter, bumped on every successful
    /// `initiate_connection` for a given session_id. The disconnect-watcher
    /// task captures the value at spawn time and bails out silently if it
    /// sees a newer generation — that's how we keep a stale watcher from
    /// double-firing `session-disconnected-{id}` after the user has
    /// already reconnected.
    pub session_generation: Arc<Mutex<HashMap<String, u64>>>,
    /// Per-tab "file operations run as root" setting: SFTP is served by
    /// `sudo <sftp-server>` instead of the plain subsystem. Survives reconnects
    /// of the same tab (the next SFTP op re-elevates) and is dropped when the
    /// tab closes.
    pub sftp_elevation: Arc<Mutex<HashMap<String, SftpElevation>>>,
    /// Per-tab root access for the Info panel's actions (services, kill,
    /// firewall rules): what worked last time, so it isn't probed or asked
    /// for again. Dropped when the tab closes, like `sftp_elevation`.
    pub root_access: Arc<Mutex<HashMap<String, RootAccess>>>,
    /// Tabs whose server refuses the user's own `docker` (not in the docker
    /// group): the Docker tab's commands go straight to root there
    /// (docker.rs). Dropped when the tab closes, like `root_access`.
    pub docker_root: Arc<Mutex<HashSet<String>>>,
    /// The saved server each session was opened for (initiate_connection,
    /// telnet_connect), and the server of each of its shell terminals: a
    /// server's own saved password is typed only into that server's
    /// terminals (type_saved_password).
    pub session_servers: Arc<Mutex<HashMap<String, i32>>>,
    /// The user each session logged in as (initiate_connection): the AI
    /// panel says who a command runs as (#116).
    pub session_users: Arc<Mutex<HashMap<String, String>>>,
    /// Per tab: one password the user typed is tried at a time, and one
    /// just turned down isn't tried again (lib.rs, typed_root_access).
    pub root_gates: Arc<Mutex<HashMap<String, Arc<Mutex<RootGate>>>>>,
    pub terminal_servers: Arc<Mutex<HashMap<String, i32>>>,
    /// Tabs whose cached SFTP session is file access over shell commands (the
    /// server has no SFTP, see `shell_sftp`): the pane says so.
    pub sftp_shell: Arc<Mutex<HashSet<String>>>,
    /// Secrets the user typed at connect time when the saved node / login / key
    /// had none (issue #30), keyed by the BASE session id (the tab). A reconnect
    /// or a dedicated `::sftp` / `::fwd` secondary reuses what the user already
    /// supplied instead of prompting again. See `PromptedSecrets`.
    pub prompted_secrets: Arc<Mutex<HashMap<String, PromptedSecrets>>>,
    /// Terminals whose shell is still opening (open_terminal,
    /// open_container_terminal). Locked before `terminal_txs` when both are.
    pub terminal_openings: Arc<Mutex<TerminalOpenings>>,
}

/// Which terminals are still opening, so a close that arrives while one is
/// (the page closed the tab before the server answered) isn't lost: opening
/// a shell waits on the server, and the terminal is registered only at the
/// end. Without this, the close found nothing, the open then registered the
/// terminal, and its shell kept running on the server with nobody to see it.
/// As local_term.rs does for Home terminals.
///
/// Each open gets a number. A close forgets the terminal's number, as does a
/// newer open of the same terminal (a reconnect): at the end, an open whose
/// number isn't the terminal's any more closes its channel and registers
/// nothing.
#[derive(Default)]
pub struct TerminalOpenings {
    last: u64,
    opening: HashMap<String, u64>,
}

impl TerminalOpenings {
    /// An open of `terminal_id` starts; keep the number for `finish`.
    pub fn begin(&mut self, terminal_id: &str) -> u64 {
        self.last += 1;
        self.opening.insert(terminal_id.to_string(), self.last);
        self.last
    }

    /// The page closed `terminal_id`: an open still under way is let go.
    pub fn close(&mut self, terminal_id: &str) {
        self.opening.remove(terminal_id);
    }

    /// The open numbered `n` ended (opened or failed). True when the
    /// terminal is still wanted from it: register it, holding this lock
    /// until it's in `terminal_txs` so a close can't come in between.
    pub fn finish(&mut self, terminal_id: &str, n: u64) -> bool {
        if self.opening.get(terminal_id) == Some(&n) {
            self.opening.remove(terminal_id);
            true
        } else {
            false
        }
    }
}

/// How an Info panel action gets root on a tab's server: the tool that let
/// the user in (`sudo` or `dzdo`) and, when it asked for one, the password —
/// in memory only, zeroised on drop, written to the tool's stdin, never on a
/// command line (as for SftpElevation).
#[derive(Clone)]
pub struct RootAccess {
    pub tool: &'static str,
    /// `None` = passwordless (`-n`).
    pub password: Option<zeroize::Zeroizing<String>>,
    /// Through a root shell (`dzdo su -`, …: the words after the tool) with
    /// the command on its stdin, where the tool's rules allow only that.
    /// `None` = the tool runs the command itself.
    pub shell: Option<&'static str>,
    /// When the password last worked. Held while one not known to work
    /// lately is tried, so the tab's other calls wait for that answer
    /// instead of each signing in with a password that may have stopped
    /// working (lib.rs, password_turn).
    pub checked: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    /// Sudo's cached sign-ins (tickets) are neither used nor left behind
    /// (`-k`): the AI's root (#120), which must never ride on a sign-in of
    /// yours, or leave one for anything else.
    pub no_tickets: bool,
}

/// A tab's gate for passwords the user types (SshState::root_gates).
#[derive(Default)]
pub struct RootGate {
    /// A salted digest of the last password sudo or dzdo turned down, and
    /// when.
    pub rejected: Option<([u8; 32], std::time::Instant)>,
}

impl RootAccess {
    pub fn new(
        tool: &'static str,
        password: Option<zeroize::Zeroizing<String>>,
        shell: Option<&'static str>,
        checked: Option<std::time::Instant>,
    ) -> Self {
        RootAccess { tool, password, shell, checked: Arc::new(tokio::sync::Mutex::new(checked)), no_tickets: false }
    }

    /// The same, using and leaving no sudo tickets (`no_tickets`).
    pub fn without_tickets(mut self) -> Self {
        self.no_tickets = true;
        self
    }
}

/// How an elevated SFTP channel is started. The sudo password (if any) lives
/// only here, in memory, zeroised on drop — never persisted, never logged,
/// never on a command line (it is written to sudo's stdin).
#[derive(Clone)]
pub struct SftpElevation {
    /// Absolute path of the server's sftp-server binary (probed once). `None`:
    /// the server has none, and file operations run as shell commands under
    /// sudo (`shell_sftp`).
    pub server_path: Option<String>,
    /// `sudo` or `dzdo`, whichever the probe found allowed (always `sudo`
    /// without an sftp-server).
    pub tool: &'static str,
    /// `None` = passwordless (`sudo -n` / `dzdo -n`).
    pub password: Option<zeroize::Zeroizing<String>>,
    /// Through a root shell, as for RootAccess.
    pub shell: Option<&'static str>,
    /// The password stopped working (sudo or dzdo said so): it isn't tried
    /// again, and root mode says so until it's turned on again with the
    /// password.
    pub password_failed: bool,
}

/// Connect-time secrets the user typed when a saved node / login / key had none
/// (issue #30). Held only in memory in `SshState::prompted_secrets`, keyed by
/// the BASE session id (the tab), so a reconnect or a dedicated `::sftp` /
/// `::fwd` secondary reuses what the user already supplied instead of asking
/// again. Every field is `Zeroizing`, so the plaintext is wiped on drop. Never
/// persisted, logged, emitted or synced. The whole entry is dropped when its
/// tab is disconnected or the profile closes (reaping a `::sftp` / `::fwd`
/// secondary keeps it — the tab is still alive), and a single secret is
/// cleared as soon as the server rejects it.
#[derive(Clone, Default)]
pub struct PromptedSecrets {
    /// Accepted password for the node's own login.
    pub password: Option<zeroize::Zeroizing<String>>,
    /// Accepted passphrase that decrypted the node's own key.
    pub passphrase: Option<zeroize::Zeroizing<String>>,
    /// Accepted password for the ProxyJump bastion's login.
    pub jump_password: Option<zeroize::Zeroizing<String>>,
    /// Accepted passphrase that decrypted the ProxyJump bastion's key.
    pub jump_passphrase: Option<zeroize::Zeroizing<String>>,
    /// Login name typed at connect time for a node saved without one
    /// (issue #54); kept once that login succeeded.
    pub username: Option<zeroize::Zeroizing<String>>,
    /// Same, for the ProxyJump bastion.
    pub jump_username: Option<zeroize::Zeroizing<String>>,
}

impl SshState {
    pub fn new() -> Self {
        Self {
            fp_txs: Arc::new(Mutex::new(HashMap::new())),
            kbi_txs: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
            jump_connections: Arc::new(Mutex::new(HashMap::new())),
            terminal_txs: Arc::new(Mutex::new(HashMap::new())),
            resize_txs: Arc::new(Mutex::new(HashMap::new())),
            sftp_sessions: Arc::new(Mutex::new(HashMap::new())),
            tunnels: Arc::new(Mutex::new(HashMap::new())),
            forwarded_targets: Arc::new(Mutex::new(HashMap::new())),
            transfer_cancels: Arc::new(Mutex::new(HashMap::new())),
            session_tunnel_specs: Arc::new(Mutex::new(HashMap::new())),
            session_generation: Arc::new(Mutex::new(HashMap::new())),
            sftp_elevation: Arc::new(Mutex::new(HashMap::new())),
            root_access: Arc::new(Mutex::new(HashMap::new())),
            docker_root: Arc::new(Mutex::new(HashSet::new())),
            session_servers: Arc::new(Mutex::new(HashMap::new())),
            session_users: Arc::new(Mutex::new(HashMap::new())),
            root_gates: Arc::new(Mutex::new(HashMap::new())),
            terminal_servers: Arc::new(Mutex::new(HashMap::new())),
            sftp_shell: Arc::new(Mutex::new(HashSet::new())),
            prompted_secrets: Arc::new(Mutex::new(HashMap::new())),
            terminal_openings: Arc::new(Mutex::new(TerminalOpenings::default())),
        }
    }
}

/// When the server last sent anything on a connection — channel data or a
/// window adjustment — on a process-wide monotonic clock in milliseconds
/// (0 = never). The session watcher reads it: on a slow link a busy channel
/// (an upload, a download) can queue the keepalive reply behind its own data
/// for longer than the probe waits, but a server that keeps sending while we
/// wait is alive.
#[derive(Clone, Default)]
pub struct LastHeard(Arc<std::sync::atomic::AtomicU64>);

impl LastHeard {
    /// The clock `stamp` writes and `heard_since` compares against. Starts at
    /// 1, so a stamp is never mistaken for "never".
    pub fn now() -> u64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64 + 1
    }

    pub fn stamp(&self) {
        self.0.store(Self::now(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether the server sent anything after `t` (a `now()` reading).
    pub fn heard_since(&self, t: u64) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed) > t
    }
}

/// One "trust this host key?" question at a time per host, port and key.
/// Opening a folder of servers behind one jump host — or several sessions
/// to the same host — put up the same prompt once per session; now the
/// first one asks and the rest take its answer. The value is `None` until
/// it's answered.
type HostKeyQuestion = (String, u16, String);
type AskedQuestions = HashMap<HostKeyQuestion, (u64, tokio::sync::watch::Receiver<Option<HostKeyAnswer>>)>;

#[derive(Clone, Copy, Debug, PartialEq)]
enum HostKeyAnswer {
    Accepted,
    Rejected,
    /// Nobody answered in time: the sessions waiting on it give up too,
    /// rather than putting the prompt up again one after another.
    Unanswered,
}
static HOST_KEY_QUESTIONS: std::sync::Mutex<Option<AskedQuestions>> = std::sync::Mutex::new(None);
static HOST_KEY_QUESTION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

enum HostKeyTurn {
    /// Another session is asking this: its answer comes through here.
    Follow(tokio::sync::watch::Receiver<Option<HostKeyAnswer>>),
    /// This session asks.
    Ask(HostKeyAsker),
}

/// The session asking a host-key question, until it's done (dropped).
struct HostKeyAsker {
    question: HostKeyQuestion,
    id: u64,
    tx: tokio::sync::watch::Sender<Option<HostKeyAnswer>>,
}

impl HostKeyAsker {
    fn answer(&self, answer: HostKeyAnswer) {
        let _ = self.tx.send(Some(answer));
    }
}

impl Drop for HostKeyAsker {
    fn drop(&mut self) {
        let mut guard = HOST_KEY_QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(map) = guard.as_mut() {
            if map.get(&self.question).is_some_and(|(id, _)| *id == self.id) {
                map.remove(&self.question);
            }
        }
    }
}

fn host_key_turn(question: HostKeyQuestion) -> HostKeyTurn {
    let mut guard = HOST_KEY_QUESTIONS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some((_, rx)) = map.get(&question) {
        return HostKeyTurn::Follow(rx.clone());
    }
    let (tx, rx) = tokio::sync::watch::channel(None);
    let id = HOST_KEY_QUESTION_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    map.insert(question.clone(), (id, rx));
    HostKeyTurn::Ask(HostKeyAsker { question, id, tx })
}

/// The answer the asking session got: `None` when it went away without one
/// (its tab closed, or it was replaced) — then the next one asks.
async fn asked_answer(mut rx: tokio::sync::watch::Receiver<Option<HostKeyAnswer>>) -> Option<HostKeyAnswer> {
    let answered = rx.wait_for(|v| v.is_some()).await.map(|v| *v);
    answered.unwrap_or_else(|_| *rx.borrow())
}

pub struct ClientHandler {
    pub app: AppHandle,
    pub session_id: String,
    /// Per-connect-attempt random nonce. Used as the key for the
    /// fingerprint-approval oneshot channel so a stale "accept" from a
    /// prior attempt (or a malicious frontend message that knows only
    /// `session_id`) cannot satisfy the prompt for a fresh connection.
    /// Echoed in the `fingerprint-prompt-{session_id}` event payload and
    /// must be sent back by the frontend in `verify_fingerprint_response`.
    pub connect_nonce: String,
    pub server_host: String,
    pub server_port: u16,
    pub db: Arc<std::sync::Mutex<Option<rusqlite::Connection>>>,
    pub fp_rx: Option<oneshot::Receiver<bool>>,
    /// Per-session map populated by R tunnels — when the server pushes a
    /// `forwarded-tcpip` channel back, we look the port up here to find the
    /// local target to bridge it to.
    pub forwarded_targets: crate::tunnel::ForwardedTargets,
    /// Fingerprint-prompt outcome, written by `check_server_key`. The connect
    /// driver reads this after `connect_stream` returns an Err so it can
    /// distinguish the three host-key cases from a generic transport drop:
    ///   -1 = no prompt fired (handshake never reached the check)
    ///    0 = prompt fired and the user rejected
    ///    1 = prompt fired and the user accepted (or fingerprint was trusted)
    ///    2 = prompt fired but timed out with no answer
    /// Without this the driver only sees russh's downstream error and can't
    /// tell "user dismissed the prompt" from "network drop", which used to
    /// surface as "Auth failed" in the UI.
    pub fp_outcome: std::sync::Arc<std::sync::atomic::AtomicI8>,
    /// Set true the instant a fingerprint prompt is emitted and cleared once
    /// the human answers (or the 90s wait times out). The connect driver
    /// watches this SAME Arc so its 15s handshake timeout bounds only the
    /// pre-prompt transport+kex phase: while a prompt is pending the wait
    /// extends to cover the human window instead of killing the prompt with a
    /// misleading "handshake stalled" error. See `drive_connect_with_prompt_timeout`.
    pub prompt_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Counts the times this attempt started its own 90s prompt window. A
    /// session that waited on another's prompt and then had to ask itself
    /// starts a fresh window, so the connect driver restarts its hard cap
    /// when this moves instead of counting from the connect's start.
    pub prompt_epoch: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// False for the dedicated `::sftp` / `::fwd` connections: the tab only
    /// shows the primary connection's fingerprint prompt, so they refuse an
    /// unknown or changed key at once instead of waiting on a prompt nobody
    /// can see.
    pub prompt_allowed: bool,
    /// The connect attempt this handler belongs to. A fingerprint prompt from
    /// an attempt that's no longer current (or that its driver gave up on) is
    /// refused silently instead of appearing over a newer attempt's.
    pub attempt: ConnectAttempt,
    /// Stamped on every packet of channel data or window adjustment the
    /// server sends; the session watcher holds a clone (see `LastHeard`).
    pub last_heard: LastHeard,
    /// What the user's known_hosts files say about this host (CAs, listed
    /// and revoked keys), when "Use settings from ~/.ssh/config" is on.
    pub(crate) host_trust: Option<Arc<crate::known_hosts::HostTrust>>,
    /// Why the host key was refused without a prompt (known_hosts said no,
    /// or a certificate nothing can check): the connect driver puts it in
    /// the failure reason instead of "not approved".
    pub(crate) host_key_refusal: Arc<std::sync::Mutex<Option<String>>>,
}

/// One connect attempt of a session: its id, the session generation it started
/// under (bumped by every new attempt and by `disconnect_session`) and whether
/// its connect driver has given up on it. A prompt from an attempt that's no
/// longer current would land on top of — or be answered instead of — a newer
/// attempt's prompt, and its failure report would flip a tab that has since
/// connected back to "failed", so both are dropped.
#[derive(Clone)]
pub struct ConnectAttempt {
    generations: Arc<Mutex<HashMap<String, u64>>>,
    session_id: String,
    generation: u64,
    abandoned: Arc<std::sync::atomic::AtomicBool>,
}

impl ConnectAttempt {
    pub fn new(generations: Arc<Mutex<HashMap<String, u64>>>, session_id: String, generation: u64) -> Self {
        Self {
            generations,
            session_id,
            generation,
            abandoned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// No newer attempt has started for the session, and it wasn't disconnected.
    pub async fn is_current(&self) -> bool {
        self.generations.lock().await.get(&self.session_id).copied().unwrap_or(0) == self.generation
    }

    /// `is_current` for synchronous callers — the attempt's log lines. If the
    /// generation map happens to be locked at this instant, say current: an
    /// extra log line is harmless, blocking a logger isn't.
    pub fn is_current_now(&self) -> bool {
        self.generations
            .try_lock()
            .map(|g| g.get(&self.session_id).copied().unwrap_or(0) == self.generation)
            .unwrap_or(true)
    }

    /// Resolves once the attempt is no longer current: the tab was
    /// disconnected or closed, or a newer attempt started. Looked at twice a
    /// second; a handshake waiting on a ProxyCommand that never answers is
    /// given up on (and the command stopped) when this resolves.
    pub async fn superseded(&self) {
        while self.is_current().await {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// The connect driver stopped waiting on this attempt (its handshake timed
    /// out), so nothing would act on an answer any more.
    pub fn abandon(&self) {
        self.abandoned.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether it still makes sense to ask the user anything for this attempt.
    pub async fn may_prompt(&self) -> bool {
        !self.abandoned.load(std::sync::atomic::Ordering::SeqCst) && self.is_current().await
    }
}

impl ClientHandler {
    fn keys_on_file(&self, fingerprint: &str) -> Result<HostKeyVerdict, String> {
        keys_on_file(&self.db, &self.server_host, self.server_port, fingerprint)
    }
}

/// What the app's trusted keys say about the key `fingerprint` for
/// host:port (judge_host_key). Fails closed: a poisoned lock, a failed
/// lookup, or no profile open (nothing to check against, and nowhere to
/// save an answer) is an `Err` with the reason, never "first time". The
/// std mutex guard is dropped before the caller's next `.await`.
fn keys_on_file(
    db: &std::sync::Mutex<Option<rusqlite::Connection>>,
    host: &str,
    port: u16,
    fingerprint: &str,
) -> Result<HostKeyVerdict, String> {
    let guard = db.lock().map_err(|_| "the trusted host keys couldn't be read; restart the app".to_string())?;
    let conn = guard.as_ref().ok_or("no profile is open, so the server's key can't be checked")?;
    judge_host_key(conn, host, port, fingerprint).map_err(|_| "the trusted host keys couldn't be read; restart the app".to_string())
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(&mut self, offered: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        // An attempt that's been superseded (a newer attempt or a disconnect)
        // or that its driver gave up on ends here, quietly: its log lines
        // would land in the newer attempt's log, its prompt over the newer
        // attempt's — and nothing would use the connection anyway.
        if !self.attempt.may_prompt().await {
            self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
            return Ok(false);
        }
        // What the user's known_hosts and the app's own trusted keys say
        // (trust_policy.rs, #43): a certificate is checked against a CA
        // known_hosts trusts for the host; a plain key below.
        let log = |msg: &str, ty: &str| {
            println!("[LOG-{}] {}", self.session_id, msg);
            if self.attempt.is_current_now() {
                let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({"msg": msg, "type": ty}));
            }
        };
        let refuse = |why: String| {
            log(&format!("Host key refused: {why}."), "error");
            if let Ok(mut r) = self.host_key_refusal.lock() {
                *r = Some(why);
            }
            // A host-key failure (not a transport / algorithm error).
            self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
            Ok(false)
        };
        // A certificate is judged by the key inside it, against the app's
        // trusted keys, as a plain key is; the prompt and what's saved on
        // approval are that key's.
        let inner_key: PublicKey;
        let (server_public_key, certificate) = match offered {
            PublicKeyOrCertificate::Certificate(cert) => {
                inner_key = PublicKey::from(cert.public_key().clone());
                (&inner_key, Some(cert))
            }
            PublicKeyOrCertificate::PublicKey { key, .. } => (key, None),
        };
        // The key's own algorithm (`ssh-rsa` for any RSA key).
        let key_type = server_public_key.algorithm().to_string();
        let key_type = key_type.as_str();
        let fp_str = host_key_fingerprint(server_public_key);

        let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
            "msg": if certificate.is_some() {
                format!("Server offered a host certificate holding the key ({}): {}", key_type, fp_str)
            } else {
                format!("Server offered key ({}): {}", key_type, fp_str)
            },
            "type": "info"
        }));

        // Every key on file for this host:port (judge_host_key): the offered
        // one among them → trusted; others on file but not it → KEY CHANGED,
        // whatever their kind; nothing on file → the first-time prompt.
        // keys_on_file drops the std mutex guard before any `.await`. A
        // poisoned mutex, a failed lookup or no open profile fails closed
        // instead of silently treating the host as unknown (which would
        // re-prompt the user and hide a possible MITM).
        let verdict = match self.keys_on_file(&fp_str) {
            Ok(v) => v,
            Err(why) => return refuse(why),
        };
        let decision = match certificate {
            Some(cert) => crate::trust_policy::decide_certificate(verdict, self.host_trust.as_deref(), cert, crate::trust_policy::now()),
            None => crate::trust_policy::decide_plain_key(verdict, self.host_trust.as_deref(), server_public_key),
        };
        let (mismatch, prior_fingerprints) =
            match decision {
                crate::trust_policy::HostKeyDecision::Trusted { why, save } => {
                    // Vouched for by known_hosts with nothing on file here:
                    // saved as if approved, so Monitoring and the changed-key
                    // warning know it from now on (#43, D4).
                    if save {
                        let saved = match self.db.lock() {
                            Ok(guard) => match guard.as_ref() {
                                Some(conn) => save_approved_host_key(conn, &self.server_host, self.server_port, &fp_str, key_type, false).is_ok(),
                                None => false,
                            },
                            Err(_) => false,
                        };
                        log(&why, "success");
                        if saved {
                            log("Saved to this app's trusted host keys.", "info");
                        }
                    } else {
                        log(&why, "success");
                    }
                    // Mark as auto-accepted (no prompt was shown) so the connect
                    // driver knows host-key wasn't the failure mode for any
                    // downstream error.
                    self.fp_outcome.store(1, std::sync::atomic::Ordering::SeqCst);
                    return Ok(true);
                }
                crate::trust_policy::HostKeyDecision::Refused(why) => return refuse(why),
                crate::trust_policy::HostKeyDecision::Ask { mismatch, prior, note } => {
                    if let Some(note) = note {
                        log(&note, "warn");
                    }
                    (mismatch, prior)
                }
            };

        if !self.prompt_allowed {
            // A dedicated SFTP / port-forwarding connection can't show a
            // prompt. The primary connection normally verifies the host just
            // before, so this only happens when the two reach hosts with
            // different keys (e.g. behind a load balancer) — refuse.
            let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                "msg": if mismatch {
                    "⚠ The host key has CHANGED — this connection was refused. Reconnect the session to review the key."
                } else {
                    "The host key isn't trusted yet and this connection can't ask — refused. Reconnect the session to review the key."
                },
                "type": "error"
            }));
            self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
            return Ok(false);
        }

        // Re-checked right before prompting: the key lookup above can take a
        // moment, and a prompt from a superseded attempt would only cover the
        // current one's.
        if !self.attempt.may_prompt().await {
            self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
            return Ok(false);
        }

        if mismatch {
            // Loud, distinct log line for the activity panel — this is the
            // SSH "REMOTE HOST IDENTIFICATION HAS CHANGED" moment.
            let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                "msg": "⚠ WARNING: Remote host key has CHANGED since you last connected. This could indicate a man-in-the-middle attack, or the server's host key was rotated. Verify out-of-band before accepting.",
                "type": "error"
            }));
        } else {
            let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                "msg": "Host fingerprint is unknown. Waiting for user approval...",
                "type": "warn"
            }));
        }

        // The same question already on screen for another session: take its
        // answer instead of asking again. If it goes unanswered, ask here.
        let question: HostKeyQuestion = (self.server_host.clone(), self.server_port, fp_str.clone());
        let asker = loop {
            let rx = match host_key_turn(question.clone()) {
                HostKeyTurn::Ask(a) => break a,
                HostKeyTurn::Follow(rx) => rx,
            };
            let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                "msg": "Another session is asking about this same host key — waiting for that answer.",
                "type": "info"
            }));
            // Waiting on a person, so the handshake timeout holds off (see below).
            self.prompt_pending.store(true, std::sync::atomic::Ordering::SeqCst);
            let wait = asked_answer(rx);
            tokio::pin!(wait);
            let answer = loop {
                tokio::select! {
                    a = &mut wait => break Some(a),
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                        if !self.attempt.may_prompt().await {
                            break None;
                        }
                    }
                }
            };
            self.prompt_pending.store(false, std::sync::atomic::Ordering::SeqCst);
            let (msg, outcome) = match answer {
                // This attempt was replaced or closed while waiting.
                None => {
                    self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
                    return Ok(false);
                }
                Some(None) => continue,
                Some(Some(HostKeyAnswer::Accepted)) => ("Host key accepted (answered in another session).", 1),
                Some(Some(HostKeyAnswer::Rejected)) => ("Host key rejected (answered in another session).", 0),
                Some(Some(HostKeyAnswer::Unanswered)) => {
                    ("Host key verification timed out (the prompt in another session wasn't answered).", 2)
                }
            };
            let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                "msg": msg,
                "type": if outcome == 1 { "success" } else { "error" }
            }));
            self.fp_outcome.store(outcome, std::sync::atomic::Ordering::SeqCst);
            return Ok(outcome == 1);
        };

        // Mark a prompt as pending BEFORE emitting it. The connect driver
        // reads this to keep its 15s handshake timeout from killing the human
        // approval window below — the timeout bounds only the pre-prompt phase.
        self.prompt_pending.store(true, std::sync::atomic::Ordering::SeqCst);
        self.prompt_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.app.emit(&format!("fingerprint-prompt-{}", self.session_id), serde_json::json!({
            "host": self.server_host,
            "port": self.server_port,
            "keyType": key_type,
            "fingerprint": fp_str,
            "mismatch": mismatch,
            "priorFingerprints": prior_fingerprints,
            // Frontend MUST echo this back via verify_fingerprint_response.
            // Without it the response is rejected. Defeats stale-channel /
            // session-id-guessing attacks against the TOFU prompt.
            "nonce": self.connect_nonce,
        }));

        let decision = if let Some(mut rx) = self.fp_rx.take() {
            // 90s is enough for a human to read the prompt, switch windows
            // to verify the fingerprint out-of-band, and click. The old 10s
            // window routinely tripped on attentive users and then surfaced
            // as a confusing "Auth failed" because russh interprets the
            // returned `false` as "client rejected the host key" and tears
            // the connection down — same error path as wrong credentials.
            // An attempt replaced or closed meanwhile stops waiting at once,
            // so sessions waiting on this answer don't sit out the 90s.
            let deadline = tokio::time::sleep(tokio::time::Duration::from_secs(90));
            tokio::pin!(deadline);
            // Ok(answer) | Ok(Err) = channel gone | Err(true) = timed out |
            // Err(false) = this attempt was replaced or closed.
            let waited: Result<Result<bool, ()>, bool> = loop {
                tokio::select! {
                    r = &mut rx => break Ok(r.map_err(|_| ())),
                    _ = &mut deadline => break Err(true),
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                        if !self.attempt.may_prompt().await {
                            break Err(false);
                        }
                    }
                }
            };
            match waited {
                // Quietly, like a superseded attempt above: the tab has moved
                // on. Its prompt goes; the sessions waiting on it ask next.
                Err(false) => {
                    let _ = self.app.emit(&format!("fingerprint-prompt-dismiss-{}", self.session_id), serde_json::json!({ "nonce": self.connect_nonce }));
                    self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
                    Ok(false)
                }
                Ok(Ok(true)) => {
                    // Save to database. If this was a mismatch the stale
                    // same-FAMILY rows are replaced in the same transaction —
                    // otherwise the next connection would see "any row matches
                    // the OLD fingerprint = trusted" because of the loop above,
                    // defeating the warning. See save_approved_host_key.
                    // For a host certificate this is the key inside it: you
                    // approved it by hand, so it's saved like a plain key. D4's
                    // "certificates aren't saved" is about saving without asking.
                    if let Ok(guard) = self.db.lock() {
                        if let Some(ref conn) = *guard {
                            let _ = save_approved_host_key(
                                conn,
                                &self.server_host,
                                self.server_port,
                                &fp_str,
                                key_type,
                                mismatch,
                            );
                        }
                    }
                    let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                        "msg": if mismatch { "New host key accepted. Old entries replaced." } else { "Host key accepted and saved." },
                        "type": "success"
                    }));
                    asker.answer(HostKeyAnswer::Accepted);
                    self.fp_outcome.store(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(true)
                }
                Ok(Ok(false)) => {
                    let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                        "msg": "Host key rejected by user.",
                        "type": "error"
                    }));
                    asker.answer(HostKeyAnswer::Rejected);
                    let _ = self.app.emit(&format!("fingerprint-prompt-dismiss-{}", self.session_id), serde_json::json!({ "nonce": self.connect_nonce }));
                    self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
                    Ok(false)
                }
                Err(_) => {
                    let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                        "msg": "Host key verification timed out (no response from user within 90 seconds).",
                        "type": "error"
                    }));
                    let _ = self.app.emit(&format!("fingerprint-prompt-dismiss-{}", self.session_id), serde_json::json!({ "nonce": self.connect_nonce }));
                    asker.answer(HostKeyAnswer::Unanswered);
                    self.fp_outcome.store(2, std::sync::atomic::Ordering::SeqCst);
                    Ok(false)
                }
                _ => {
                    let _ = self.app.emit(&format!("session-log-{}", self.session_id), serde_json::json!({
                        "msg": "Host key verification aborted.",
                        "type": "error"
                    }));
                    let _ = self.app.emit(&format!("fingerprint-prompt-dismiss-{}", self.session_id), serde_json::json!({ "nonce": self.connect_nonce }));
                    self.fp_outcome.store(0, std::sync::atomic::Ordering::SeqCst);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        };
        // The human window has resolved (accepted / rejected / timed out) —
        // clear the flag so the driver's prompt-aware timeout settles promptly.
        self.prompt_pending.store(false, std::sync::atomic::Ordering::SeqCst);
        // Sessions waiting on this answer have it (or, unanswered, ask next).
        drop(asker);
        decision
    }

    /// Inbound channel from a server-side `tcpip_forward` we set up earlier
    /// (remote tunnel, the SSH `-R` shape). The server has accepted an
    /// outside connection on `connected_port`; we just need to bridge that
    /// channel to a local TCP socket pointed at the user's chosen target.
    ///
    /// Since russh 0.62 the open is answered through `reply`: nothing is
    /// confirmed to the server (and no data can flow) until `accept()`. The
    /// decision is made OFF the protocol task — `bridge_forwarded_channel`
    /// dials the local target first and only then accepts, or rejects with
    /// `ConnectFailed` — so the outside connector gets a real refusal instead
    /// of an accepted-then-immediately-closed channel. An unknown port is
    /// rejected right here. (A dropped `reply` also rejects, so no path can
    /// leave the server waiting on an unanswered open.)
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let entry = self.forwarded_targets.lock().await.get(&connected_port).cloned();
        match entry {
            Some(entry) => {
                // Cap concurrent inbound bridges per R tunnel so a hostile or
                // runaway server can't exhaust us (tasks, sockets, memory) by
                // spamming forwarded-tcpip opens. try_acquire is non-blocking:
                // at the cap we refuse the open rather than queue it (an
                // unbounded queue is the same resource-exhaustion problem). The
                // permit rides into the bridge and frees when the bridge ends.
                match std::sync::Arc::clone(&entry.conn_limiter).try_acquire_owned() {
                    Ok(permit) => {
                        // Spawn the bridge so we don't hold up russh's protocol
                        // task. `bridge_forwarded_channel` does the local
                        // connect, answers the open, then runs the always-drained
                        // bidirectional pump, plus bumps the tunnel's connection
                        // counter for the UI.
                        tokio::spawn(crate::tunnel::bridge_forwarded_channel(entry, channel, reply, permit));
                    }
                    Err(_) => {
                        drop(channel);
                        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                    }
                }
            }
            None => {
                // No tunnel registered for this port — refuse the open. The
                // never-accepted channel has no registered receiver, so
                // dropping it can't stall the connection.
                drop(channel);
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
            }
        }
        Ok(())
    }

    // This is a client: the only channels it accepts are the ones it asked
    // for (a shell, an exec, an SFTP subsystem) and the forwarded-tcpip opens
    // for port forwards it set up (above). Every OTHER channel the server
    // tries to open — a session, a direct or streamlocal forward, an X11 or
    // agent forward, or anything unknown — is refused. russh's defaults
    // ACCEPT these, and an accepted-then-dropped channel lets a hostile
    // server keep opening channels the client tracks but never uses.
    async fn server_channel_open_session(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_direct_tcpip(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_direct_streamlocal(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _socket_path: &str,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _socket_path: &str,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    async fn server_channel_open_unknown(
        &mut self,
        _channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
        Ok(())
    }

    // The next three only note that the server is talking (see `LastHeard`).
    // russh hands the data itself to the channel's own stream as well; the
    // default implementations do nothing.
    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        _data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.last_heard.stamp();
        Ok(())
    }

    async fn extended_data(
        &mut self,
        _channel: russh::ChannelId,
        _ext: u32,
        _data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.last_heard.stamp();
        Ok(())
    }

    async fn window_adjusted(
        &mut self,
        _channel: russh::ChannelId,
        _new_size: u32,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.last_heard.stamp();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // No profile open: the key can't be checked, so it's refused with that
    // reason, never asked about as if it were new.
    #[test]
    fn no_open_profile_means_the_key_cant_be_checked() {
        let none = std::sync::Mutex::new(None);
        assert_eq!(keys_on_file(&none, "h", 22, "FP").unwrap_err(), "no profile is open, so the server's key can't be checked");
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE known_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT, port INTEGER, fingerprint TEXT, key_type TEXT);")
            .unwrap();
        let open = std::sync::Mutex::new(Some(conn));
        assert_eq!(keys_on_file(&open, "h", 22, "FP").unwrap(), HostKeyVerdict::New);
        // A broken table fails closed too.
        open.lock().unwrap().as_ref().unwrap().execute_batch("DROP TABLE known_hosts;").unwrap();
        assert!(keys_on_file(&open, "h", 22, "FP").is_err());
    }

    #[test]
    fn a_terminal_closed_while_opening_is_not_registered() {
        let mut t = TerminalOpenings::default();
        let n = t.begin("s-term-0");
        t.close("s-term-0");
        assert!(!t.finish("s-term-0", n));
        // Nothing left behind for the next open of that terminal.
        let m = t.begin("s-term-0");
        assert!(t.finish("s-term-0", m));
        assert!(t.opening.is_empty());
    }

    #[test]
    fn a_terminal_left_open_is_registered_once() {
        let mut t = TerminalOpenings::default();
        let n = t.begin("a");
        let other = t.begin("b");
        assert!(t.finish("a", n));
        // A late close of another terminal, or a second finish, changes nothing.
        t.close("c");
        assert!(!t.finish("a", n));
        assert!(t.finish("b", other));
    }

    #[test]
    fn a_newer_open_of_the_same_terminal_wins() {
        let mut t = TerminalOpenings::default();
        let old = t.begin("a");
        let new = t.begin("a");
        assert!(!t.finish("a", old));
        assert!(t.finish("a", new));
        // Closed between two opens: only the open after the close counts.
        let first = t.begin("a");
        t.close("a");
        let second = t.begin("a");
        assert!(!t.finish("a", first));
        assert!(t.finish("a", second));
    }

    #[tokio::test]
    async fn one_host_key_question_at_a_time() {
        let q = || ("hk-test.example".to_string(), 22u16, "SHA256:abc".to_string());
        let HostKeyTurn::Ask(asker) = host_key_turn(q()) else { panic!("first one asks") };
        let HostKeyTurn::Follow(rx) = host_key_turn(q()) else { panic!("second one follows") };
        // Another key on the same host is its own question.
        let HostKeyTurn::Ask(_other) = host_key_turn(("hk-test.example".into(), 22, "SHA256:xyz".into())) else {
            panic!("a different key asks for itself")
        };
        let waiting = tokio::spawn(asked_answer(rx));
        asker.answer(HostKeyAnswer::Accepted);
        assert_eq!(waiting.await.unwrap(), Some(HostKeyAnswer::Accepted));
        drop(asker);

        // Gone without an answer: the follower hears None and asks itself.
        let HostKeyTurn::Ask(asker) = host_key_turn(q()) else { panic!("asks again once the first is done") };
        let HostKeyTurn::Follow(rx) = host_key_turn(q()) else { panic!("follows") };
        let waiting = tokio::spawn(asked_answer(rx));
        drop(asker);
        assert_eq!(waiting.await.unwrap(), None);
        assert!(matches!(host_key_turn(q()), HostKeyTurn::Ask(_)));
    }

    #[test]
    fn last_heard_counts_only_what_arrived_after_the_question() {
        let heard = LastHeard::default();
        let before = LastHeard::now();
        assert!(!heard.heard_since(0), "nothing heard yet");
        std::thread::sleep(std::time::Duration::from_millis(5));
        heard.stamp();
        assert!(heard.heard_since(before));
        std::thread::sleep(std::time::Duration::from_millis(5));
        let asked_at = LastHeard::now();
        assert!(!heard.heard_since(asked_at), "an older stamp doesn't answer a newer question");
        // A clone (what the watcher keeps) sees the handler's stamps.
        let watcher = heard.clone();
        std::thread::sleep(std::time::Duration::from_millis(5));
        heard.stamp();
        assert!(watcher.heard_since(asked_at));
    }

    /// The stored `known_hosts` format must stay byte-identical to what
    /// russh-keys 0.40 wrote: un-padded base64 SHA-256 of the key blob, no
    /// `SHA256:` prefix. Guards the upgrade against silently invalidating
    /// every pinned host.
    #[test]
    fn host_key_fingerprint_matches_legacy_format() {
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let seed = [7u8; 32];
        let key = PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from(
            russh::keys::ssh_key::private::Ed25519PrivateKey::from_bytes(&seed),
        ));
        let public = key.public_key();
        let fp = host_key_fingerprint(public);
        let expected = base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(Sha256::digest(public.to_bytes().unwrap()));
        assert_eq!(fp, expected);
        assert!(!fp.starts_with("SHA256:"));
    }

    #[test]
    fn rsa_signature_names_share_one_family() {
        assert_eq!(key_type_family("rsa-sha2-512"), "ssh-rsa");
        assert_eq!(key_type_family("rsa-sha2-256"), "ssh-rsa");
        assert_eq!(key_type_family("ssh-rsa"), "ssh-rsa");
        assert_eq!(key_type_family("ssh-ed25519"), "ssh-ed25519");
    }

    // Fixture keys generated with `ssh-keygen`; expected strings are
    // `ssh-keygen -lf <key>.pub` output WITHOUT its `SHA256:` prefix — the
    // exact format russh-keys 0.40's `PublicKey::fingerprint()` produced
    // (BASE64_NOPAD(SHA-256(wire blob))), i.e. what known_hosts rows hold.
    // The ed25519 and ecdsa values were also re-derived with the real
    // russh-keys 0.40.1 crate (identical output).
    const ED25519: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDUtueMG6aRvJ5KvL2MdRTSJvjovFlJSkevE6M2STKJo fixture";
    const ED25519_FP_OLD_FORMAT: &str = "xmRWREanofXGEC0I34CTheE8ie4mzdYWeJicXVl4EvY";
    const ECDSA_P256: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBHV+twrHbUzZsVehTQv4Nwncx45MzHiBuQHKD7m+V8LHNlLsEqFoh26DlW3X6QUOg5qyzlZP68I48UZL+mZUEi4= fixture";
    const ECDSA_P256_FP_OLD_FORMAT: &str = "sv9PmqdzTou9hi/MJot5kkVRp1RlTzczpZBY9K1oNpY";
    const RSA_2048: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQDV+BvzoBQWSFqUzFzroKlwFUmPsTDHZt3JKIX+S7eHR9CaLFSaUdxPgpf/s9BF5Ivm8sV4NiKGUbWzbwItcTzNk3JYEidewYxXb1B3drU7lt5aXWb2WkLQ3LNh+h7oyrJS9y6juLi4KYDceNwACb7gPJHiQYx187b9lfQhPr7YBAm6p17EQPp0p5mfdjXRR4fp/9bdGfjo32H+gdYlKUstcWM3h7/9t4D8DqVP+AGWdOav30dvqJkZecV81g7vCtFqGbpo6xw03yQ71BQJCcvaquptGtjJiA59iHql5D/KKM0zRN+r5lxNuduQIasBGc+GGdhioEqzmeYTI250PSAJ fixture";
    const RSA_2048_FP_OLD_FORMAT: &str = "nxrXMG0NyO+gdFkxT/vcYRg0no5urQPoiP7rc75QHow";

    fn key(openssh: &str) -> PublicKey {
        PublicKey::from_openssh(openssh).expect("fixture key parses")
    }

    #[test]
    fn fixture_fingerprints_match_russh_keys_0_40_format() {
        let fp = host_key_fingerprint(&key(ED25519));
        assert_eq!(fp, ED25519_FP_OLD_FORMAT);
        // Guard against the ssh-key Display format (or padding) sneaking back in.
        assert!(!fp.starts_with("SHA256:"));
        assert!(!fp.ends_with('='));
        assert_eq!(host_key_fingerprint(&key(ECDSA_P256)), ECDSA_P256_FP_OLD_FORMAT);
        assert_eq!(host_key_fingerprint(&key(RSA_2048)), RSA_2048_FP_OLD_FORMAT);
    }

    #[test]
    fn fingerprint_is_the_unprefixed_body_of_ssh_key_display() {
        let k = key(ED25519);
        let display = k.fingerprint(HashAlg::Sha256).to_string();
        assert_eq!(display, format!("SHA256:{}", ED25519_FP_OLD_FORMAT));
        assert_eq!(display.strip_prefix("SHA256:"), Some(host_key_fingerprint(&k).as_str()));
    }

    #[test]
    fn key_type_names_reported_by_russh_0_63() {
        // New rows store these; key_type_family must fold them with the names
        // russh 0.40 stored for the same keys.
        assert_eq!(key(ED25519).algorithm().to_string(), "ssh-ed25519");
        assert_eq!(key(ECDSA_P256).algorithm().to_string(), "ecdsa-sha2-nistp256");
        assert_eq!(key(RSA_2048).algorithm().to_string(), "ssh-rsa");
    }

    #[test]
    fn rsa_spellings_are_one_family_regardless_of_case_or_cert_suffix() {
        let fam = key_type_family("ssh-rsa");
        for spelling in [
            "rsa-sha2-256",
            "rsa-sha2-512",
            "RSA-SHA2-512",
            " ssh-rsa ",
            "rsa-sha2-512-cert-v01@openssh.com",
            "ssh-rsa-cert-v01@openssh.com",
        ] {
            assert_eq!(key_type_family(spelling), fam, "{spelling}");
        }
    }

    #[test]
    fn other_families_stay_distinct() {
        let all = [
            key_type_family("ssh-ed25519"),
            key_type_family("ecdsa-sha2-nistp256"),
            key_type_family("ecdsa-sha2-nistp384"),
            key_type_family("ecdsa-sha2-nistp521"),
            key_type_family("rsa-sha2-512"),
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "families {a} and {b} must differ");
                }
            }
        }
        assert_eq!(key_type_family("ssh-ed25519-cert-v01@openssh.com"), all[0]);
    }

    /// known_hosts exactly as `setup_master_db_inner` creates it.
    fn known_hosts_db(rows: &[(&str, u16, &str, Option<&str>)]) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE known_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT, port INTEGER, fingerprint TEXT, key_type TEXT);",
        )
        .unwrap();
        for (host, port, fp, kt) in rows {
            conn.execute(
                "INSERT INTO known_hosts (host, port, fingerprint, key_type) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![host, port, fp, kt],
            )
            .unwrap();
        }
        conn
    }

    fn fingerprints(conn: &rusqlite::Connection, host: &str, port: u16) -> Vec<(String, Option<String>)> {
        let mut stmt = conn
            .prepare("SELECT fingerprint, key_type FROM known_hosts WHERE host=?1 AND port=?2 ORDER BY fingerprint")
            .unwrap();
        stmt.query_map(rusqlite::params![host, port], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// B3: accepting a CHANGED key must retire every old fingerprint for the
    /// host, the RSA ones pinned under a russh-0.40 signature name included
    /// (once left trusted next to the new key) and those of other kinds: the
    /// user has just said this is the server's identity now.
    #[test]
    fn accepting_a_changed_key_retires_every_key_on_file_for_the_host() {
        let conn = known_hosts_db(&[
            ("h", 22, "OLD_RSA_512", Some("rsa-sha2-512")),
            ("h", 22, "OLD_RSA_256", Some("rsa-sha2-256")),
            ("h", 22, "LEGACY", None),
            ("h", 22, "ED", Some("ssh-ed25519")),
            ("other", 22, "OTHER_HOST", Some("rsa-sha2-512")),
            ("h", 2222, "OTHER_PORT", Some("ssh-rsa")),
        ]);
        save_approved_host_key(&conn, "h", 22, "NEW_RSA", "ssh-rsa", true).unwrap();
        assert_eq!(fingerprints(&conn, "h", 22), vec![("NEW_RSA".to_string(), Some("ssh-rsa".to_string()))]);
        assert_eq!(fingerprints(&conn, "other", 22).len(), 1, "other hosts untouched");
        assert_eq!(fingerprints(&conn, "h", 2222).len(), 1, "other ports untouched");
    }

    // Any key on file that isn't the offered one makes it a change, whatever
    // its kind: a man-in-the-middle without the pinned Ed25519 key used to get
    // the first-time prompt by offering an ECDSA key. The keys trusted before
    // come with their kinds; nothing on file is the first-time prompt.
    #[test]
    fn a_key_of_another_kind_is_a_change_too() {
        let conn = known_hosts_db(&[
            ("h", 22, "ED", Some("ssh-ed25519")),
            ("h", 22, "LEGACY", None),
            ("h", 2222, "OTHER_PORT", Some("ssh-rsa")),
        ]);
        assert_eq!(judge_host_key(&conn, "h", 22, "ED").unwrap(), HostKeyVerdict::Known);
        assert_eq!(
            judge_host_key(&conn, "h", 22, "ECDSA").unwrap(),
            HostKeyVerdict::Changed { prior: vec!["ED (ssh-ed25519)".to_string(), "LEGACY".to_string()] }
        );
        assert_eq!(judge_host_key(&conn, "new", 22, "ECDSA").unwrap(), HostKeyVerdict::New);
    }

    #[test]
    fn the_kinds_of_key_on_file_are_per_host_and_port() {
        let conn = known_hosts_db(&[
            ("h", 22, "A", Some("rsa-sha2-512")),
            ("h", 22, "B", Some("ssh-rsa")),
            ("h", 22, "C", Some("ecdsa-sha2-nistp256")),
            ("h", 22, "D", None),
            ("h", 2222, "E", Some("ssh-ed25519")),
        ]);
        assert_eq!(known_host_families(&conn, "h", 22), vec!["ecdsa-sha2-nistp256", "ssh-rsa"]);
        assert!(known_host_families(&conn, "x", 22).is_empty());
    }

    #[test]
    fn accepting_an_unknown_key_only_adds_a_row() {
        let conn = known_hosts_db(&[("h", 22, "RSA", Some("rsa-sha2-512"))]);
        save_approved_host_key(&conn, "h", 22, "ED", "ssh-ed25519", false).unwrap();
        assert_eq!(fingerprints(&conn, "h", 22).len(), 2);
    }

    #[test]
    fn a_failed_save_rolls_back_so_the_old_key_stays_on_file() {
        let conn = known_hosts_db(&[("h", 22, "OLD_RSA", Some("rsa-sha2-512"))]);
        conn.execute_batch(
            "CREATE TRIGGER no_insert BEFORE INSERT ON known_hosts BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        )
        .unwrap();
        assert!(save_approved_host_key(&conn, "h", 22, "NEW_RSA", "ssh-rsa", true).is_err());
        assert_eq!(
            fingerprints(&conn, "h", 22),
            vec![("OLD_RSA".to_string(), Some("rsa-sha2-512".to_string()))],
            "the DELETE must be rolled back — never zero fingerprints on file"
        );
        // And the connection is usable again (no transaction left open).
        conn.execute_batch("DROP TRIGGER no_insert;").unwrap();
        save_approved_host_key(&conn, "h", 22, "NEW_RSA", "ssh-rsa", true).unwrap();
        assert_eq!(fingerprints(&conn, "h", 22).len(), 1);
    }

    /// B2 regression, against a real (in-process) SSH connection: a large paste
    /// that must wait for the server's window, while the server floods output.
    /// The pre-0.63-port single-`select!` loop stopped reading output while it
    /// awaited the write, the channel queue filled, russh's protocol task
    /// blocked delivering into it and so never processed the WINDOW_ADJUST the
    /// write waited on — a deadlock that froze the whole connection.
    #[tokio::test]
    async fn pty_pump_keeps_draining_output_while_a_large_paste_waits_for_window() {
        use crate::ssh_test_server::{connect, TestServer};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        let server = TestServer::default();
        let received = Arc::clone(&server.received);
        // Small server-side window: the paste below needs ~16 WINDOW_ADJUSTs,
        // each of which our protocol task must be free to process.
        let session = connect(server, |c| c.window_size = 64 * 1024, crate::build_ssh_client_config())
            .await
            .expect("in-process SSH connection");
        let channel = session.channel_open_session().await.unwrap();
        // The "shell" floods output for the whole test (think `yes`, a build log).
        channel.exec(false, "flood 1073741824").await.unwrap();

        let (tx, rx) = mpsc::channel::<TerminalCommand>(32);
        let (_resize_tx, resize_rx) = tokio::sync::watch::channel(PtySize { cols: 80, rows: 24 });
        let output = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&output);
        let pump = tokio::spawn(pty_pump(channel, rx, resize_rx, Vec::new(), move |buf: &mut Vec<u8>| {
            sink.fetch_add(buf.len(), Ordering::SeqCst);
            buf.clear();
        }));

        const PASTE: usize = 1024 * 1024;
        tx.send(TerminalCommand::Data(vec![b'p'; PASTE])).await.unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            while received.load(Ordering::SeqCst) < PASTE {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the paste must reach the server while output keeps flowing (no deadlock)");
        assert!(output.load(Ordering::SeqCst) > 0, "output must keep being drained meanwhile");

        // Closing the tab (all senders dropped) ends the pump.
        drop(tx);
        tokio::time::timeout(Duration::from_secs(10), pump)
            .await
            .expect("pump must stop once the terminal is closed")
            .unwrap();
    }

    /// A key file signs a certificate login as an agent would: the signature,
    /// with the RSA hash the server asked for, appended to the data as an SSH
    /// string.
    #[test]
    fn key_file_signer_appends_a_signature_with_the_asked_hash() {
        use russh::keys::signature::Verifier;
        use russh::keys::ssh_encoding::Decode;
        use russh::keys::ssh_key::{private::RsaKeypair, Signature};
        const DATA: &[u8] = b"session id and request";
        let rsa = PrivateKey::from(RsaKeypair::random(&mut rand::rng(), 2048).unwrap());
        let ed = PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
        for (key, hash, name) in [
            (&rsa, Some(HashAlg::Sha512), "rsa-sha2-512"),
            (&rsa, Some(HashAlg::Sha256), "rsa-sha2-256"),
            (&rsa, None, "ssh-rsa"),
            (&ed, None, "ssh-ed25519"),
        ] {
            let mut data = DATA.to_vec();
            sign_appended(key, hash, &mut data).unwrap();
            let (signed, mut rest) = data.split_at(DATA.len());
            assert_eq!(signed, DATA);
            let blob = Vec::<u8>::decode(&mut rest).unwrap();
            assert!(rest.is_empty(), "{name}: nothing after the signature");
            let signature = Signature::decode(&mut blob.as_slice()).unwrap();
            assert_eq!(signature.algorithm().as_str(), name);
            Verifier::verify(key.public_key(), DATA, &signature).unwrap();
        }
    }
}


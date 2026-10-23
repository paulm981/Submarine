//! Running an allowed command on a shared server: on a channel of its own on
//! the tab's SSH connection, as the Info panel and the Docker tab's logs do,
//! so the terminals stay yours. The limits (#32 row 11):
//!
//! - **No terminal attached,** and nothing on stdin: programs that want one
//!   (`top`, `vim`, a password prompt) end or wait, and pagers (`less`,
//!   `git`'s, `systemctl`'s) don't start. PAGER and its kin are set to `cat`
//!   too, where the server accepts them.
//! - **An output cap:** past `cap` bytes the command is stopped.
//! - **A timeout:** past it the command is stopped. It counts from the start,
//!   opening the channel included.
//! - **Stop:** a stop signal (Stop, unsharing, the AI tab closing, AI tools
//!   turned off) stops it at once.
//!
//! Stopped for any reason, the command is ended on the server too: a KILL
//! signal and a close for its channel, sent here before `run` returns. A
//! russh `Channel` sends nothing to the server when it's dropped, so the
//! command must never be left to a dropped task to end.

use crate::ssh_manager::RootAccess;
use russh::client;
use russh::ChannelMsg;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use zeroize::Zeroizing;

/// Asked for, best effort: most servers only accept LANG and LC_*.
const PAGER_ENV: &[(&str, &str)] = &[("PAGER", "cat"), ("GIT_PAGER", "cat"), ("SYSTEMD_PAGER", "cat"), ("MANPAGER", "cat")];

/// How long sending the signal, and the close, may take at most each (a
/// connection that's going away).
const END_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone)]
pub(crate) struct Exec {
    /// stdout and stderr, as they came, up to the cap.
    pub output: Vec<u8>,
    pub exit_status: Option<u32>,
    pub exit_signal: Option<String>,
    /// Stopped at the cap.
    pub cut: bool,
    /// Stopped at the timeout.
    pub timed_out: bool,
    /// Stopped by the stop signal.
    pub stopped: bool,
}

/// How a run that got as far as a channel ended.
enum End {
    /// The command ended, or the server closed the channel.
    Finished,
    Cut,
    TimedOut,
    Stopped,
    Failed(String),
}

/// Resolves once `stop` is set, or once nothing can set it any more: every
/// sender gone means whoever wanted the command is gone too.
async fn stop_asked(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|s| *s).await;
}

/// Run `command` and collect its output, until it ends, `limit` passes, the
/// output passes `cap`, or `stop` is set (see the module's notes).
pub(crate) async fn run<H: client::Handler>(
    handle: &tokio::sync::Mutex<client::Handle<H>>,
    command: &str,
    limit: Duration,
    cap: usize,
    mut stop: watch::Receiver<bool>,
) -> Result<Exec, String> {
    let deadline = tokio::time::Instant::now() + limit;
    let mut out = Exec::default();
    // Opening the channel waits for the connection's lock and the server:
    // under the timeout and Stop too. Nothing runs on the server before the
    // channel exists.
    let open = async {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| format!("Couldn't open a channel to the server: {e}"))
    };
    let mut channel = tokio::select! {
        biased;
        _ = stop_asked(&mut stop) => {
            out.stopped = true;
            return Ok(out);
        }
        r = tokio::time::timeout_at(deadline, open) => match r {
            Ok(r) => r?,
            Err(_) => {
                out.timed_out = true;
                return Ok(out);
            }
        },
    };

    // From here on, every way out ends the channel below.
    let start = async {
        for (k, v) in PAGER_ENV {
            let _ = channel.set_env(false, *k, *v).await;
        }
        channel.exec(true, command.as_bytes()).await.map_err(|e| format!("Couldn't start the command: {e}"))?;
        channel.eof().await.map_err(|e| format!("Couldn't start the command: {e}"))
    };
    let started = tokio::select! {
        biased;
        _ = stop_asked(&mut stop) => Err(End::Stopped),
        r = tokio::time::timeout_at(deadline, start) => match r {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(End::Failed(e)),
            Err(_) => Err(End::TimedOut),
        },
    };
    let end = match started {
        Err(end) => end,
        Ok(()) => tokio::select! {
            biased;
            _ = stop_asked(&mut stop) => End::Stopped,
            r = tokio::time::timeout_at(deadline, collect(&mut channel, &mut out, cap)) => r.unwrap_or(End::TimedOut),
        },
    };
    if !matches!(end, End::Finished) {
        // Servers that take signals (OpenSSH 8.1 and later) end it now; most
        // others when its channel closes, the rest when it next writes.
        let _ = tokio::time::timeout(END_WAIT, channel.signal(russh::Sig::KILL)).await;
    }
    let _ = tokio::time::timeout(END_WAIT, channel.close()).await;
    match end {
        End::Finished => {}
        End::Cut => out.cut = true,
        End::TimedOut => out.timed_out = true,
        End::Stopped => out.stopped = true,
        End::Failed(e) => return Err(e),
    }
    Ok(out)
}

async fn collect(channel: &mut russh::Channel<client::Msg>, out: &mut Exec, cap: usize) -> End {
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { ref data } | ChannelMsg::ExtendedData { ref data, .. } => {
                let room = cap.saturating_sub(out.output.len());
                out.output.extend_from_slice(&data[..data.len().min(room)]);
                if data.len() > room {
                    return End::Cut;
                }
            }
            ChannelMsg::ExitStatus { exit_status } => out.exit_status = Some(exit_status),
            ChannelMsg::ExitSignal { signal_name, .. } => out.exit_signal = Some(format!("{signal_name:?}")),
            ChannelMsg::Failure => return End::Failed("The server refused to run the command.".into()),
            ChannelMsg::Close => return End::Finished,
            _ => {}
        }
    }
    End::Finished
}

// ---------------------------------------------------------------------------
// As root (#120)
// ---------------------------------------------------------------------------
//
// A request with `as_root` runs through sudo or dzdo, with the same code as
// SFTP root mode, the Info panel and the Docker tab (lib.rs): the tool is
// found as they find it (dzdo, then sudo, then a root shell such as
// `dzdo su -`), and a password goes only to the tool's own prompt
// (ROOT_EXEC_SCRIPT and root_feed: a random prompt, `-S -p "" -v`, one
// attempt per password). What this AI tab has for root on the server is
// kept by mod.rs in the server's place in its list, never in SshState, so
// the AI tab and the server's own tab never share a root password.
//
// The limits are the same as for any command: no terminal, nothing on
// stdin but the password, the time limit (counted from the start, the look
// for root included), the output cap, and Stop.

/// How a root command gets root.
pub(crate) enum RootHow {
    /// What this AI tab found, or was given, for the server before.
    Kept(Arc<RootAccess>),
    /// The password you typed on the card. Wiped when dropped.
    Typed(Zeroizing<String>),
    /// Nothing yet: look for root without a password.
    Find,
}

/// How a root command went.
pub(crate) enum RootRun {
    /// It ran, or was stopped, cut or timed out (as `Exec` says). With the
    /// root access it found or was given, when that's new (not `Kept`).
    Ran(Exec, Option<Arc<RootAccess>>),
    /// The server wants a password this request didn't have. Nothing ran.
    /// The tool that asked, if known.
    NeedPassword(Option<&'static str>),
    /// sudo or dzdo turned the password down: one attempt. Nothing ran.
    WrongPassword(Option<&'static str>),
    /// Root couldn't be had, or the command couldn't start. Why, for you
    /// (the panel and the audit log) and for the AI: words of Submarine's
    /// own for the AI, never the server's.
    Failed { you: String, ai: String },
}

/// What the AI's root command runs: its words, where it's a plain command
/// (trust::plain_words: so a sudoers rule for that one program allows it),
/// else `sh -c <command>`.
pub(crate) fn root_words(command: &str) -> Vec<String> {
    super::trust::plain_words(command).unwrap_or_else(|| vec!["sh".into(), "-c".into(), command.to_string()])
}

/// A [SUDO] code from lib.rs as RootRun::Failed's words.
fn failed(code: &str) -> RootRun {
    let (you, ai): (String, &str) = if code == crate::SUDO_NO_SUDO {
        ("Neither sudo nor dzdo is on this server.".into(), "Couldn't run it as root: the server has neither sudo nor dzdo.")
    } else if code == crate::SUDO_NOT_ALLOWED {
        (
            "sudo and dzdo don't let this login run that as root here.".into(),
            "Couldn't run it as root: sudo and dzdo don't allow this login to run that on this server.",
        )
    } else if code == crate::SUDO_REQUIRETTY {
        (
            "sudo here only works in a terminal (requiretty), and the AI's commands run without one.".into(),
            "Couldn't run it as root: sudo on this server only works in a terminal, and Submarine runs commands without one.",
        )
    } else if code == crate::SUDO_NEED_PASSWORD {
        ("The server asked for a root password.".into(), "Couldn't run it as root: the server asked for a root password.")
    } else {
        let detail = code.strip_prefix(crate::SUDO_FAILED).map(|d| d.trim_start_matches(':').trim()).unwrap_or(code);
        (format!("sudo or dzdo couldn't run it: {detail}"), "Couldn't run it as root: sudo or dzdo couldn't start it. The user can see why in Submarine.")
    };
    RootRun::Failed { you, ai: ai.to_string() }
}

/// The look for root, under the stop signal and the time limit.
enum Look<T> {
    Done(T),
    Stopped,
    TimedOut,
}

async fn within<T>(stop: &mut watch::Receiver<bool>, deadline: tokio::time::Instant, f: impl std::future::Future<Output = T>) -> Look<T> {
    tokio::select! {
        biased;
        _ = stop_asked(stop) => Look::Stopped,
        r = tokio::time::timeout_at(deadline, f) => match r {
            Ok(v) => Look::Done(v),
            Err(_) => Look::TimedOut,
        },
    }
}

/// Run `command` as root on the connection `handle`, getting root as `how`
/// says, and collect its output as `run` does. `started` hears once the
/// tool has taken the password (or needed none) and the command started.
pub(crate) async fn run_root<H: client::Handler>(
    handle: &Arc<tokio::sync::Mutex<client::Handle<H>>>,
    how: RootHow,
    command: &str,
    limit: Duration,
    cap: usize,
    mut stop: watch::Receiver<bool>,
    started: Option<tokio::sync::oneshot::Sender<()>>,
) -> RootRun {
    let deadline = tokio::time::Instant::now() + limit;
    let words = root_words(command);
    let stopped = |timed_out: bool| RootRun::Ran(Exec { stopped: !timed_out, timed_out, ..Exec::default() }, None);
    // How root is had: kept, found with the password you typed (one
    // sign-in: a wrong one ends it), or found without one.
    let (access, new) = match how {
        RootHow::Kept(a) => (a, false),
        RootHow::Typed(pw) => match within(&mut stop, deadline, crate::find_root_with_password(handle, &words, &pw)).await {
            Look::Stopped => return stopped(false),
            Look::TimedOut => return stopped(true),
            Look::Done(Ok((tool, shell))) => (Arc::new(RootAccess::new(tool, Some(pw), shell, Some(std::time::Instant::now()))), true),
            Look::Done(Err(e)) if e == crate::SUDO_WRONG_PASSWORD => return RootRun::WrongPassword(None),
            Look::Done(Err(e)) => return failed(&e),
        },
        RootHow::Find => match within(&mut stop, deadline, crate::find_root_without_password(handle, &words)).await {
            Look::Stopped => return stopped(false),
            Look::TimedOut => return stopped(true),
            Look::Done(Ok(a)) => (Arc::new(a), true),
            Look::Done(Err((e, tool))) if e == crate::SUDO_NEED_PASSWORD => return RootRun::NeedPassword(tool),
            Look::Done(Err((e, _))) => return failed(&e),
        },
    };
    let tool = access.tool;
    let new_access = || new.then(|| access.clone());

    let mut out = Exec::default();
    let open = async {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| format!("Couldn't open a channel to the server: {e}"))
    };
    let mut channel = match within(&mut stop, deadline, open).await {
        Look::Stopped => return stopped(false),
        Look::TimedOut => return stopped(true),
        Look::Done(Err(e)) => return RootRun::Failed { you: e.clone(), ai: format!("Couldn't run it: {e}") },
        Look::Done(Ok(c)) => c,
    };
    // From here on, every way out ends the channel below.
    for (k, v) in PAGER_ENV {
        let _ = channel.set_env(false, *k, *v).await;
    }
    let start = crate::root_channel_start(&mut channel, &access, &words, false, None);
    let (end, marker) = match within(&mut stop, deadline, start).await {
        Look::Stopped => (End::Stopped, None),
        Look::TimedOut => (End::TimedOut, None),
        Look::Done(Err(e)) => {
            let _ = tokio::time::timeout(END_WAIT, channel.close()).await;
            return if e == crate::SUDO_WRONG_PASSWORD {
                RootRun::WrongPassword(Some(tool))
            } else if e == crate::SUDO_NEED_PASSWORD {
                RootRun::NeedPassword(Some(tool))
            } else {
                failed(&e)
            };
        }
        Look::Done(Ok((marker, early))) => {
            if let Some(s) = started {
                let _ = s.send(());
            }
            let room = cap.min(early.len());
            out.output.extend_from_slice(&early[..room]);
            let end = if early.len() > cap {
                End::Cut
            } else {
                tokio::select! {
                    biased;
                    _ = stop_asked(&mut stop) => End::Stopped,
                    r = tokio::time::timeout_at(deadline, collect(&mut channel, &mut out, cap)) => r.unwrap_or(End::TimedOut),
                }
            };
            (end, marker)
        }
    };
    if !matches!(end, End::Finished) {
        let _ = tokio::time::timeout(END_WAIT, channel.signal(russh::Sig::KILL)).await;
    }
    let _ = tokio::time::timeout(END_WAIT, channel.close()).await;
    match end {
        End::Finished => {}
        End::Cut => out.cut = true,
        End::TimedOut => out.timed_out = true,
        End::Stopped => out.stopped = true,
        End::Failed(e) => return RootRun::Failed { you: e.clone(), ai: format!("Couldn't run it: {e}") },
    }
    // Through a root shell, the output starts after the marker line, which
    // says who the shell is: the root shell's own start-up comes before it.
    if let Some(marker) = marker {
        let text = String::from_utf8_lossy(&out.output).into_owned();
        match text.find(marker) {
            Some(at) => {
                let rest = &text[at + marker.len()..];
                let (uid, after) = rest.split_once('\n').unwrap_or((rest, ""));
                if uid.trim() != "0" {
                    return failed(crate::SUDO_NOT_ALLOWED);
                }
                out.output = after.as_bytes().to_vec();
            }
            None if out.stopped || out.timed_out || out.cut => out.output.clear(),
            None => return failed(&crate::classify_tool_output(&text)),
        }
    } else if access.password.is_none() && access.shell.is_none() && out.exit_status.is_some_and(|c| c != 0) {
        // `-l` says yes without a password where any of the login's rules
        // has NOPASSWD (sudo's `listpw`), even for a command that needs one;
        // running it says so, and nothing ran.
        if crate::tool_answer(&String::from_utf8_lossy(&out.output)).as_deref() == Some(crate::SUDO_NEED_PASSWORD) {
            return RootRun::NeedPassword(Some(tool));
        }
    }
    RootRun::Ran(out, new_access())
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use crate::ssh_test_server::{self, ShellOpts, TestClient, TestServer};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    pub(crate) type Conn = Arc<tokio::sync::Mutex<client::Handle<TestClient>>>;

    /// A test server running commands with this machine's sh, and its count
    /// of commands still running. Its signal and close end the `sh` only,
    /// as sshd's signal does, so tests that watch a command end use `exec`:
    /// a child of sh would keep the output open after sh ends.
    pub(crate) async fn server_with(opts: ShellOpts) -> (Conn, Arc<AtomicUsize>) {
        let srv = TestServer::with_shell(opts);
        let running = srv.running.clone();
        let h = ssh_test_server::connect(srv, |_| {}, client::Config::default()).await.unwrap();
        (Arc::new(tokio::sync::Mutex::new(h)), running)
    }

    pub(crate) async fn server() -> Conn {
        server_with(ShellOpts::default()).await.0
    }

    /// A stop signal that's never given (its sender kept alive).
    fn no_stop() -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        std::mem::forget(tx);
        rx
    }

    pub(crate) async fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
        let t = std::time::Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(10), "waited for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn output_and_exit_status() {
        let h = server().await;
        let r = run(&h, "echo out; echo err >&2; exit 3", Duration::from_secs(10), 1024, no_stop()).await.unwrap();
        let text = String::from_utf8_lossy(&r.output);
        assert!(text.contains("out") && text.contains("err"), "{text}");
        assert_eq!(r.exit_status, Some(3));
        assert!(!r.cut && !r.timed_out && !r.stopped);
    }

    #[tokio::test]
    async fn no_terminal_and_nothing_on_stdin() {
        let h = server().await;
        let r = run(&h, "if [ -t 0 ] || [ -t 1 ]; then echo tty; else echo none; fi; cat; echo done", Duration::from_secs(10), 1024, no_stop())
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&r.output).trim(), "none\ndone");
    }

    #[tokio::test]
    async fn stops_at_the_timeout() {
        let (h, running) = server_with(ShellOpts::default()).await;
        let t = std::time::Instant::now();
        let r = run(&h, "echo started; exec sleep 30", Duration::from_millis(800), 1024, no_stop()).await.unwrap();
        assert!(r.timed_out);
        assert!(t.elapsed() < Duration::from_secs(5));
        assert!(String::from_utf8_lossy(&r.output).contains("started"));
        wait_until("the command to end on the server", || running.load(Ordering::SeqCst) == 0).await;
    }

    #[tokio::test]
    async fn stops_at_the_cap() {
        let h = server().await;
        let r = run(&h, "yes 0123456789", Duration::from_secs(10), 4096, no_stop()).await.unwrap();
        assert!(r.cut);
        assert_eq!(r.output.len(), 4096);
    }

    /// Stop ends the command on the server, not only here: on a server that
    /// ends commands when their channel closes, and on one (like Dropbear)
    /// that only ends them on a signal.
    #[tokio::test]
    async fn stop_ends_the_command_on_the_server() {
        for keep_running_on_close in [false, true] {
            let (h, running) = server_with(ShellOpts { keep_running_on_close, ..ShellOpts::default() }).await;
            let (stop_tx, stop) = watch::channel(false);
            let h2 = h.clone();
            let task = tokio::spawn(async move { run(&h2, "echo started; exec sleep 30", Duration::from_secs(60), 1024, stop).await });
            wait_until("the command to start", || running.load(Ordering::SeqCst) == 1).await;
            stop_tx.send(true).unwrap();
            let r = tokio::time::timeout(Duration::from_secs(5), task).await.expect("run returned").unwrap().unwrap();
            assert!(r.stopped && !r.timed_out);
            wait_until("the command to end on the server", || running.load(Ordering::SeqCst) == 0).await;
        }
    }

    /// Whoever wanted it going away (every stop sender dropped) stops it too.
    #[tokio::test]
    async fn nobody_waiting_stops_it() {
        let (h, running) = server_with(ShellOpts::default()).await;
        let (stop_tx, stop) = watch::channel(false);
        let h2 = h.clone();
        let task = tokio::spawn(async move { run(&h2, "exec sleep 30", Duration::from_secs(60), 1024, stop).await });
        wait_until("the command to start", || running.load(Ordering::SeqCst) == 1).await;
        drop(stop_tx);
        assert!(tokio::time::timeout(Duration::from_secs(5), task).await.expect("run returned").unwrap().unwrap().stopped);
        wait_until("the command to end on the server", || running.load(Ordering::SeqCst) == 0).await;
    }

    /// Stopped before it started, nothing runs.
    #[tokio::test]
    async fn stopped_before_it_starts_runs_nothing() {
        let (h, running) = server_with(ShellOpts::default()).await;
        let (stop_tx, stop) = watch::channel(true);
        let r = run(&h, "sleep 30", Duration::from_secs(60), 1024, stop).await.unwrap();
        assert!(r.stopped);
        drop(stop_tx);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(running.load(Ordering::SeqCst), 0);
    }
}

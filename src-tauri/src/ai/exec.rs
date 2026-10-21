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

use russh::client;
use russh::ChannelMsg;
use std::time::Duration;
use tokio::sync::watch;

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

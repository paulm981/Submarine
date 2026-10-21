//! Running an allowed command on a shared server: on a channel of its own on
//! the tab's SSH connection, as the Info panel and the Docker tab's logs do,
//! so the terminals stay yours. The limits (#32 row 11):
//!
//! - **No terminal attached,** and nothing on stdin: programs that want one
//!   (`top`, `vim`, a password prompt) end or wait, and pagers (`less`,
//!   `git`'s, `systemctl`'s) don't start. PAGER and its kin are set to `cat`
//!   too, where the server accepts them.
//! - **An output cap:** past `cap` bytes the command is stopped.
//! - **A timeout:** past it the command is stopped.

use russh::client;
use russh::ChannelMsg;
use std::time::Duration;

/// Asked for, best effort: most servers only accept LANG and LC_*.
const PAGER_ENV: &[(&str, &str)] = &[("PAGER", "cat"), ("GIT_PAGER", "cat"), ("SYSTEMD_PAGER", "cat"), ("MANPAGER", "cat")];

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
}

pub(crate) async fn run<H: client::Handler>(
    handle: &tokio::sync::Mutex<client::Handle<H>>,
    command: &str,
    limit: Duration,
    cap: usize,
) -> Result<Exec, String> {
    let mut channel = {
        let h = handle.lock().await;
        h.channel_open_session().await.map_err(|e| format!("Couldn't open a channel to the server: {e}"))?
    };
    for (k, v) in PAGER_ENV {
        let _ = channel.set_env(false, *k, *v).await;
    }
    channel.exec(true, command.as_bytes()).await.map_err(|e| format!("Couldn't start the command: {e}"))?;
    channel.eof().await.map_err(|e| format!("Couldn't start the command: {e}"))?;
    let mut out = Exec::default();
    let finished = tokio::time::timeout(limit, collect(&mut channel, &mut out, cap)).await;
    match finished {
        Ok(Err(e)) => return Err(e),
        Ok(Ok(())) => {}
        Err(_) => out.timed_out = true,
    }
    if out.timed_out || out.cut {
        // Servers that take signals (OpenSSH 8.1 and later) end it now; the
        // rest when it next writes to the closed channel.
        let _ = channel.signal(russh::Sig::KILL).await;
    }
    let _ = channel.close().await;
    Ok(out)
}

async fn collect(channel: &mut russh::Channel<client::Msg>, out: &mut Exec, cap: usize) -> Result<(), String> {
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { ref data } | ChannelMsg::ExtendedData { ref data, .. } => {
                let room = cap.saturating_sub(out.output.len());
                out.output.extend_from_slice(&data[..data.len().min(room)]);
                if data.len() > room {
                    out.cut = true;
                    return Ok(());
                }
            }
            ChannelMsg::ExitStatus { exit_status } => out.exit_status = Some(exit_status),
            ChannelMsg::ExitSignal { signal_name, .. } => out.exit_signal = Some(format!("{signal_name:?}")),
            ChannelMsg::Failure => return Err("The server refused to run the command.".into()),
            ChannelMsg::Close => return Ok(()),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use crate::ssh_test_server::{self, ShellOpts, TestClient, TestServer};
    use std::sync::Arc;

    pub(crate) async fn server() -> Arc<tokio::sync::Mutex<client::Handle<TestClient>>> {
        let h = ssh_test_server::connect(TestServer::with_shell(ShellOpts::default()), |_| {}, client::Config::default())
            .await
            .unwrap();
        Arc::new(tokio::sync::Mutex::new(h))
    }

    #[tokio::test]
    async fn output_and_exit_status() {
        let h = server().await;
        let r = run(&h, "echo out; echo err >&2; exit 3", Duration::from_secs(10), 1024).await.unwrap();
        let text = String::from_utf8_lossy(&r.output);
        assert!(text.contains("out") && text.contains("err"), "{text}");
        assert_eq!(r.exit_status, Some(3));
        assert!(!r.cut && !r.timed_out);
    }

    #[tokio::test]
    async fn no_terminal_and_nothing_on_stdin() {
        let h = server().await;
        let r = run(&h, "if [ -t 0 ] || [ -t 1 ]; then echo tty; else echo none; fi; cat; echo done", Duration::from_secs(10), 1024)
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&r.output).trim(), "none\ndone");
    }

    #[tokio::test]
    async fn stops_at_the_timeout() {
        let h = server().await;
        let t = std::time::Instant::now();
        let r = run(&h, "echo started; sleep 30", Duration::from_millis(800), 1024).await.unwrap();
        assert!(r.timed_out);
        assert!(t.elapsed() < Duration::from_secs(5));
        assert!(String::from_utf8_lossy(&r.output).contains("started"));
    }

    #[tokio::test]
    async fn stops_at_the_cap() {
        let h = server().await;
        let r = run(&h, "yes 0123456789", Duration::from_secs(10), 4096).await.unwrap();
        assert!(r.cut);
        assert_eq!(r.output.len(), 4096);
    }
}

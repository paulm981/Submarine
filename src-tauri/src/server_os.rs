//! Which OS a server runs, read over its session once it is connected, for
//! the badge beside it in the session tree and the server grid.
//!
//! Kept in the `servers.os` column, which is device-local: it is not one of
//! the synced columns, and writing it holds the sync triggers back, so a
//! detection never counts as an edit to push.

use crate::{DbState, SshState};
use rusqlite::{params, OptionalExtension};

/// A server moved to another host or port is asked again: what the old one
/// reported says nothing about the new one. A trigger, so an edit that
/// arrives by sync clears it too. Dropped and re-created on every vault open,
/// like the sync triggers, so its definition is always this build's.
pub(crate) const RESET_TRIGGER: &str = "DROP TRIGGER IF EXISTS servers_os_reset;
  CREATE TRIGGER servers_os_reset AFTER UPDATE OF host, port ON servers FOR EACH ROW
  WHEN NEW.os IS NOT NULL AND (NEW.host IS NOT OLD.host OR NEW.port IS NOT OLD.port)
  BEGIN UPDATE servers SET os = NULL WHERE id = NEW.id; END;";

/// os-release on Linux and the BSDs that have it, then uname, then Windows'
/// `ver` (cmd.exe, the default shell of Windows' OpenSSH server). The login
/// shell reads it as it is (run_probe), so it is one line every one of them
/// reads the same way: sh, csh, tcsh and fish, and cmd.exe. No `2>`: csh
/// reads `cat f 2>/dev/null` as `cat f 2`, its output to /dev/null. Only
/// stdout is read, so what fails says nothing.
const PROBE: &str = "cat /etc/os-release || uname -s || ver";

/// How long the probe gets, sent and answered.
const PROBE_SECS: u64 = 15;

/// What of its answer is read.
const PROBE_MAX: usize = 64 * 1024;

/// PROBE's stdout, up to PROBE_MAX, on a channel of its own. Not through
/// the app's `run_script` (`sh -s`): a Windows server has no sh, and the
/// badge is how it says it is Windows. So the login shell gets the line as
/// it is (remote_sh::Reader::LoginShell).
async fn run_probe<H: russh::client::Handler>(session: &tokio::sync::RwLock<russh::client::Handle<H>>) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let channel = crate::shell_sftp::open_session_channel(session).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(PROBE_SECS);
    crate::remote_sh::send_command(&channel, crate::remote_sh::Reader::LoginShell, PROBE, deadline).await?;
    let mut stream = channel.into_stream();
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    let read = async {
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = PROBE_MAX - out.len();
                    out.extend_from_slice(&buf[..n.min(room)]);
                    if out.len() >= PROBE_MAX {
                        break;
                    }
                }
            }
        }
    };
    tokio::time::timeout_at(deadline, read).await.map_err(|_| format!("the OS probe timed out after {}s", PROBE_SECS))?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// The OS IDs the frontend has a badge for (src/components/OsBadge.tsx). An
/// ID not among them is stored as the first of its ID_LIKE that is.
const KNOWN: &[&str] = &[
    "ubuntu", "debian", "rhel", "centos", "rocky", "almalinux", "fedora", "ol", "amzn", "sles",
    "opensuse", "opensuse-leap", "alpine", "arch", "linuxmint", "raspbian", "freebsd", "linux",
    "macos", "windows",
];

fn clean_id(raw: &str) -> Option<String> {
    let id = raw.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase();
    (!id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
        .then_some(id)
}

/// The OS ID in the probe's output, or None when it says nothing usable.
fn parse_os(out: &str) -> Option<String> {
    let field = |key: &str| {
        out.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()))
    };
    if let Some(id) = field("ID=").and_then(|v| clean_id(&v)) {
        if KNOWN.contains(&id.as_str()) {
            return Some(id);
        }
        let like = field("ID_LIKE=").unwrap_or_default();
        if let Some(known) = like.split_whitespace().filter_map(clean_id).find(|l| KNOWN.contains(&l.as_str())) {
            return Some(known);
        }
        return Some(id);
    }
    if out.contains("Microsoft Windows") {
        return Some("windows".into());
    }
    match out.lines().map(str::trim).find(|l| !l.is_empty())? {
        "Linux" => Some("linux".into()),
        "Darwin" => Some("macos".into()),
        "FreeBSD" => Some("freebsd".into()),
        _ => None,
    }
}

/// Remember `os` for the server, if the row still has the `host` and `port`
/// the probe was made with. True when the stored value changed.
fn store_os(conn: &rusqlite::Connection, server_id: i32, host: &str, port: i64, os: &str) -> Result<bool, String> {
    let current: Option<Option<String>> = conn
        .query_row(
            "SELECT os FROM servers WHERE id=?1 AND host=?2 AND port=?3",
            params![server_id, host, port],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| format!("[DATABASE] SERVER_OS_QUERY_FAILED: {}", e))?;
    match current {
        // Gone, or no longer that host and port: nothing to store.
        None => Ok(false),
        Some(current) if current.as_deref() == Some(os) => Ok(false),
        Some(_) => {
            // Held back from the sync triggers: what the server reports
            // is not an edit to push.
            crate::sync_merge::with_merge_flag(conn, |c| {
                c.execute("UPDATE servers SET os=?1 WHERE id=?2", params![os, server_id])
                    .map_err(|e| format!("[DATABASE] SERVER_OS_UPDATE_FAILED: {}", e))
            })?;
            Ok(true)
        }
    }
}

/// Read the OS of the server behind `session_id` and remember it. Returns the
/// OS ID, or None when the server's answer said nothing usable.
///
/// `host` and `port` are the server's as the page knew them when it asked:
/// the answer is stored only while the row still has both, so a server edited
/// (or a profile switched) during the probe doesn't take another host's or
/// port's OS.
#[tauri::command]
pub async fn detect_server_os(
    ssh: tauri::State<'_, SshState>,
    db: tauri::State<'_, DbState>,
    session_id: String,
    server_id: i32,
    host: String,
    port: i64,
) -> Result<Option<String>, String> {
    let session = {
        let connections = ssh.connections.lock().await;
        connections.get(&session_id).map(std::sync::Arc::clone).ok_or_else(|| "Session not connected".to_string())?
    };
    let out = run_probe(&session).await?;
    let Some(os) = parse_os(&out) else { return Ok(None) };
    let changed = {
        let conn_guard = db.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        store_os(conn, server_id, &host, port, &os)?
    };
    if changed {
        crate::save_vault_internal(&db)?;
    }
    Ok(Some(os))
}

#[cfg(test)]
mod tests {
    use super::{parse_os, store_os, RESET_TRIGGER};

    /// The probe through each login shell this machine has, as a server
    /// runs it: the line as it is, read by that shell (sh, csh, tcsh and
    /// fish all name the OS), never `sh -s`, which a Windows server lacks.
    #[cfg(unix)]
    #[tokio::test]
    async fn every_login_shell_names_the_os() {
        use crate::ssh_test_server::{connect, ShellOpts, TestServer};
        use std::sync::Arc;
        let mut ran = Vec::new();
        for login in [&["sh", "-c"][..], &["bash", "-c"], &["dash", "-c"], &["csh", "-c"], &["tcsh", "-c"], &["fish", "-c"]] {
            let found = std::process::Command::new("sh").args(["-c", &format!("command -v {} >/dev/null", login[0])]).status();
            if !found.is_ok_and(|s| s.success()) {
                eprintln!("skipped {}: not installed", login[0]);
                continue;
            }
            let server = TestServer::with_shell(ShellOpts {
                path: None,
                cwd: None,
                login: login.iter().map(|s| s.to_string()).collect(),
                keep_running_on_close: false,
            });
            let commands = Arc::clone(&server.commands);
            let s = tokio::sync::RwLock::new(connect(server, |_| {}, crate::build_ssh_client_config()).await.unwrap());
            let out = super::run_probe(&s).await.unwrap();
            assert!(parse_os(&out).is_some(), "{}: {out}", login[0]);
            assert_eq!(*commands.lock().unwrap(), vec![super::PROBE.to_string()], "{}", login[0]);
            ran.push(login[0]);
        }
        assert!(ran.contains(&"sh"), "{ran:?}");
    }

    /// One line, nothing csh, fish or cmd.exe read another way.
    #[test]
    fn the_probe_reads_the_same_in_every_shell() {
        assert!(!super::PROBE.contains(['\n', '\'', '"', '!', '\\', '$', '>', '<', '`', ';']), "{}", super::PROBE);
    }
    use rusqlite::Connection;

    #[test]
    fn a_new_host_or_port_forgets_the_os() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, host TEXT, port INTEGER, os TEXT);").unwrap();
        conn.execute_batch(RESET_TRIGGER).unwrap();
        conn.execute_batch(RESET_TRIGGER).unwrap(); // every vault open runs it
        conn.execute("INSERT INTO servers VALUES (1, 'web', 'a.example', 22, 'ubuntu')", []).unwrap();
        let os = || conn.query_row("SELECT os FROM servers WHERE id = 1", [], |r| r.get::<_, Option<String>>(0)).unwrap();

        conn.execute("UPDATE servers SET name = 'web1', host = 'a.example', port = 22", []).unwrap();
        assert_eq!(os().as_deref(), Some("ubuntu"), "same host and port");
        conn.execute("UPDATE servers SET host = 'b.example'", []).unwrap();
        assert_eq!(os(), None);
        conn.execute("UPDATE servers SET os = 'rocky'", []).unwrap();
        conn.execute("UPDATE servers SET port = 2222", []).unwrap();
        assert_eq!(os(), None);
    }

    #[test]
    fn a_probe_of_another_port_is_not_stored() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE servers(id INTEGER PRIMARY KEY, host TEXT, port INTEGER, os TEXT);
             CREATE TABLE sync_flags(key TEXT PRIMARY KEY, val INTEGER);
             INSERT INTO servers VALUES (1, 'a.example', 2222, NULL);",
        )
        .unwrap();
        let os = || conn.query_row("SELECT os FROM servers WHERE id = 1", [], |r| r.get::<_, Option<String>>(0)).unwrap();
        // Asked while the server was on port 22; it has moved since.
        assert_eq!(store_os(&conn, 1, "a.example", 22, "ubuntu"), Ok(false));
        assert_eq!(os(), None);
        assert_eq!(store_os(&conn, 1, "b.example", 2222, "ubuntu"), Ok(false));
        assert_eq!(os(), None);
        assert_eq!(store_os(&conn, 1, "a.example", 2222, "ubuntu"), Ok(true));
        assert_eq!(os().as_deref(), Some("ubuntu"));
        assert_eq!(store_os(&conn, 1, "a.example", 2222, "ubuntu"), Ok(false), "already stored");
    }

    #[test]
    fn os_release_names_the_distribution() {
        assert_eq!(parse_os("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n").as_deref(), Some("ubuntu"));
        assert_eq!(parse_os("NAME=\"Rocky Linux\"\nID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n").as_deref(), Some("rocky"));
        assert_eq!(parse_os("ID=\"rhel\"\nVERSION_ID=\"8.9\"\n").as_deref(), Some("rhel"));
    }

    #[test]
    fn an_unknown_distribution_falls_back_to_what_it_is_like() {
        assert_eq!(parse_os("ID=mycorp\nID_LIKE=\"rhel fedora\"\n").as_deref(), Some("rhel"));
        assert_eq!(parse_os("ID=mycorp\n").as_deref(), Some("mycorp"));
    }

    #[test]
    fn without_os_release_uname_or_ver_answers() {
        assert_eq!(parse_os("Linux\n").as_deref(), Some("linux"));
        assert_eq!(parse_os("Darwin\n").as_deref(), Some("macos"));
        assert_eq!(parse_os("'cat' is not recognized\r\n\r\nMicrosoft Windows [Version 10.0.20348.2340]\r\n").as_deref(), Some("windows"));
        assert_eq!(parse_os("").as_deref(), None);
        assert_eq!(parse_os("something else\n").as_deref(), None);
    }

    #[test]
    fn only_a_plain_id_is_kept() {
        assert_eq!(parse_os("ID=\"$(reboot)\"\n").as_deref(), None);
        assert_eq!(parse_os(&format!("ID={}\n", "x".repeat(40))).as_deref(), None);
    }
}

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
/// arrives by sync clears it too.
pub(crate) const RESET_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS servers_os_reset AFTER UPDATE OF host, port ON servers FOR EACH ROW
  WHEN NEW.os IS NOT NULL AND (NEW.host IS NOT OLD.host OR NEW.port IS NOT OLD.port)
  BEGIN UPDATE servers SET os = NULL WHERE id = NEW.id; END;";

/// os-release on Linux and the BSDs that have it, then uname, then Windows'
/// `ver` (cmd.exe, the default shell of Windows' OpenSSH server).
const PROBE: &str = "cat /etc/os-release 2>/dev/null || uname -s 2>/dev/null || ver";

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

/// Read the OS of the server behind `session_id` and remember it. Returns the
/// OS ID, or None when the server's answer said nothing usable.
///
/// `host` is the server's host as the page knew it when it asked: the answer
/// is stored only while the row still has it, so a server edited (or a
/// profile switched) during the probe doesn't take another host's OS.
#[tauri::command]
pub async fn detect_server_os(
    ssh: tauri::State<'_, SshState>,
    db: tauri::State<'_, DbState>,
    session_id: String,
    server_id: i32,
    host: String,
) -> Result<Option<String>, String> {
    let out = crate::run_exec_capture(&ssh, &session_id, PROBE, 15).await?;
    let Some(os) = parse_os(&out) else { return Ok(None) };
    let changed = {
        let conn_guard = db.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        let current: Option<Option<String>> = conn
            .query_row("SELECT os FROM servers WHERE id=?1 AND host=?2", params![server_id, host], |r| r.get(0))
            .optional()
            .map_err(|e| format!("[DATABASE] SERVER_OS_QUERY_FAILED: {}", e))?;
        match current {
            // Gone, or no longer that host: nothing to store.
            None => false,
            Some(current) if current.as_deref() == Some(os.as_str()) => false,
            Some(_) => {
                // The merge flag holds the sync triggers back, as during a
                // sync merge: what the server reports is not an edit to push.
                let _ = conn.execute("INSERT INTO sync_flags(key, val) VALUES('merge', 1) ON CONFLICT(key) DO UPDATE SET val = 1", []);
                let res = conn.execute("UPDATE servers SET os=?1 WHERE id=?2", params![os, server_id]);
                let _ = conn.execute("UPDATE sync_flags SET val = 0 WHERE key = 'merge'", []);
                res.map_err(|e| format!("[DATABASE] SERVER_OS_UPDATE_FAILED: {}", e))?;
                true
            }
        }
    };
    if changed {
        crate::save_vault_internal(&db)?;
    }
    Ok(Some(os))
}

#[cfg(test)]
mod tests {
    use super::{parse_os, RESET_TRIGGER};
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

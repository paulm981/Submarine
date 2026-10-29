//! The fork's rules for saved passwords (#121, after the original project's
//! #241 and #242), kept out of the original project's code, which calls in
//! here from one-line hooks.
//!
//! - E2. Remote Desktop Connection gets only the server's own saved password,
//!   never a saved login's: that login is for every server that uses it, and
//!   in the app's `.rdp` file and the app's own memory of it, it would be out
//!   of Submarine's hands.
//! - E3. A VNC password typed at its prompt is saved only into the server
//!   itself: no "Save password" tick when the server has a saved login. VNC
//!   sends its password with weak protection and doesn't check the server's
//!   identity, so a login's password mustn't go through it (vnc_open never
//!   uses one).
//! - E4. In a shared profile, a password saved reaches everyone in it: the
//!   tick starts off, and a line under it says so.

use rusqlite::Connection;
use zeroize::Zeroizing;

/// E4: the line under "Save password" in a shared profile.
pub const SHARED_PROFILE_NOTE: &str = "Off because this profile is shared: a password saved here goes to everyone in it.";

/// Whether the server logs in with a saved login rather than its own
/// password (as `rdp_saved_password` and `store_prompted_password` read it).
fn uses_saved_login(conn: &Connection, server_id: i32) -> bool {
    conn.query_row(
        "SELECT 1 FROM servers WHERE id = ?1 AND COALESCE(auth_type, 'vault') = 'vault' AND credential_id IS NOT NULL",
        [server_id],
        |_| Ok(()),
    )
    .is_ok()
}

/// E2: what Remote Desktop Connection may get of `saved`, the password the
/// server logs in with: the server's own, never its saved login's.
pub fn rdp_app_password(conn: &Connection, server_id: i32, saved: Option<Zeroizing<String>>) -> Option<Zeroizing<String>> {
    saved.filter(|_| !uses_saved_login(conn, server_id))
}

/// E3: whether a VNC password typed at the prompt may be offered for saving.
pub fn vnc_may_save(conn: &Connection, server_id: i32) -> bool {
    !uses_saved_login(conn, server_id)
}

/// E4: whether a password saved now reaches other people: the open profile
/// is shared.
pub fn saving_reaches_others(conn: &Connection) -> bool {
    crate::approvals::profile_is_shared(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE servers(id INTEGER PRIMARY KEY, auth_type TEXT, credential_id INTEGER);
             CREATE TABLE sync_meta(key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO servers VALUES (1, 'custom_pass', NULL), (2, 'vault', 7), (3, NULL, 7), (4, 'vault', NULL), (5, 'custom_pass', 7);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn the_rdp_app_gets_a_servers_own_password_only() {
        let conn = db();
        let pw = || Some(Zeroizing::new("pw".to_string()));
        assert_eq!(rdp_app_password(&conn, 1, pw()).as_deref().map(String::as_str), Some("pw"), "its own");
        assert!(rdp_app_password(&conn, 2, pw()).is_none(), "a saved login's");
        assert!(rdp_app_password(&conn, 3, pw()).is_none(), "a saved login's, from before auth types");
        assert!(rdp_app_password(&conn, 4, pw()).is_some(), "no login linked: nothing of one to keep back");
        assert!(rdp_app_password(&conn, 5, pw()).is_some(), "its own, a stale link aside");
        assert!(rdp_app_password(&conn, 1, None).is_none());
    }

    #[test]
    fn a_vnc_password_is_saved_only_into_the_server() {
        let conn = db();
        assert!(vnc_may_save(&conn, 1));
        assert!(!vnc_may_save(&conn, 2));
        assert!(!vnc_may_save(&conn, 3));
        assert!(vnc_may_save(&conn, 99), "a server not there saves nothing anyway");
    }

    #[test]
    fn a_shared_profile_is_told_apart() {
        let conn = db();
        assert!(!saving_reaches_others(&conn));
        conn.execute("INSERT INTO sync_meta VALUES ('share_id', 'abc')", []).unwrap();
        assert!(saving_reaches_others(&conn));
    }
}

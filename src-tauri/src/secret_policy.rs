//! The fork's rules for saved passwords (#121, after the original project's
//! #241 and #242), kept out of the original project's code, which calls in
//! here from one-line hooks.
//!
//! - E3. A VNC password typed at its prompt is saved only into the server
//!   itself: no "Save password" tick when the server has a saved login. VNC
//!   sends its password with weak protection and doesn't check the server's
//!   identity, so a login's password mustn't go through it (vnc_open never
//!   uses one).
//! - E4. In a shared profile, a password saved reaches everyone in it: the
//!   tick starts off, and a line under it says so.
//! - A typed password is saved only if the server is still the one it was
//!   typed for (`save_fingerprint`): in a shared profile someone else can
//!   change it while you log in, linking it to a saved login other servers
//!   use, or pointing it at another host.
//! - Only the first password question of a keyboard-interactive exchange is
//!   offered for saving (`kbi_round`), and only for the login name the server
//!   has saved (`may_save_for_login`).
//! - E5: a shared profile never opens by itself on Windows (quick unlock's
//!   "open automatically", the original project's #275): anyone using that
//!   Windows account would get every member's saved logins. It can't be turned
//!   on for one (`may_open_by_itself`), sharing a profile turns it off
//!   (`stop_opening_by_itself`), and a shared one that opens that way anyway
//!   has it turned off.
//!
//! The tests at the end run against the app's own tables
//! (`sync_engine_tests::device`) and go through the same steps the app does
//! (`save_typed_password`, `rdp_saved_password`), so a sync that renames a
//! column or reroutes a step shows up here.

use rusqlite::Connection;

/// E5: why "Open automatically" is refused for a shared profile.
pub const SHARED_AUTO_OPEN: &str = "[QUICK_UNLOCK] SHARED: a shared profile doesn't open by itself, as anyone using this Windows account would get everyone's saved logins. Windows Hello still works.";

/// E5: whether the open profile may open by itself (not when it's shared).
pub fn may_open_by_itself(conn: &Connection) -> bool {
    !crate::approvals::profile_is_shared(conn)
}

/// E5: the profile `name` has just been shared: it stops opening by itself.
pub fn stop_opening_by_itself(app: &tauri::AppHandle, name: &str) {
    crate::quick_unlock::forget_auto(app, name);
}

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

/// The server a typed password is for, as a number: its address, port, login
/// name, how it logs in, its saved login, its jump host and its kind. Taken
/// when the prompt is shown and again just before saving; None when the
/// server isn't there.
pub fn save_fingerprint(conn: &Connection, server_id: i32) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    type Row = (Option<String>, Option<i64>, Option<String>, Option<String>, Option<i64>, Option<i64>, Option<String>);
    let row: Row = conn
        .query_row(
            "SELECT host, port, username, auth_type, credential_id, jump_host_id, protocol FROM servers
             WHERE id = ?1 AND COALESCE(deleted, 0) = 0",
            [server_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    row.hash(&mut h);
    Some(h.finish())
}

/// Whether a password typed for `server_id` may be saved now: the server is
/// still as it was when the prompt was shown (`fingerprint`), and for VNC
/// (`vnc`) it still has no saved login (E3).
pub fn may_save_now(conn: &Connection, server_id: i32, fingerprint: u64, vnc: bool) -> Result<(), String> {
    if save_fingerprint(conn, server_id) != Some(fingerprint) {
        return Err("the server was changed while you logged in, so nothing was saved".into());
    }
    if vnc && !vnc_may_save(conn, server_id) {
        return Err("this VNC server now has a saved login, and a VNC password is saved only into the server".into());
    }
    Ok(())
}

/// One keyboard-interactive round: whether its "Save password" tick may be
/// offered, and whether an answer kept from an earlier round must be dropped.
/// Only the first password question offers it. A server that asks again (in
/// its own words: "Password:", "Enter your Okta password") mustn't have the
/// second answer saved over the first, into a saved login other servers use
/// too. `asked` is the exchange's own: whether a password question came
/// already.
pub fn kbi_round(password_question: bool, asked: &mut bool) -> (bool, bool) {
    let first = password_question && !*asked;
    let again = password_question && *asked;
    *asked |= password_question;
    (first, again)
}

/// Whether a password typed for this login may be saved: only for the login
/// name the server has saved. One typed at "Login as", now or earlier in the
/// tab, isn't saved, so neither is its password: the next login could be
/// under another name.
pub fn may_save_for_login(name_typed_now: bool, saved_user: &str) -> bool {
    !name_typed_now && !saved_user.trim().is_empty()
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
    fn a_vnc_password_is_saved_only_into_the_server() {
        let conn = db();
        assert!(vnc_may_save(&conn, 1));
        assert!(!vnc_may_save(&conn, 2));
        assert!(!vnc_may_save(&conn, 3));
        assert!(vnc_may_save(&conn, 99), "a server not there saves nothing anyway");
    }

    #[test]
    fn a_password_is_saved_only_for_the_server_it_was_typed_for() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE servers(id INTEGER PRIMARY KEY, host TEXT, port INTEGER, username TEXT, auth_type TEXT,
                                  credential_id INTEGER, jump_host_id INTEGER, protocol TEXT, deleted INTEGER NOT NULL DEFAULT 0);
             INSERT INTO servers VALUES (1, 'db-01', 22, 'root', 'custom_pass', NULL, NULL, 'ssh', 0),
                                        (2, 'vnc-01', 5900, NULL, 'custom_pass', NULL, NULL, 'vnc', 0);",
        )
        .unwrap();
        let fp = save_fingerprint(&conn, 1).unwrap();
        assert!(may_save_now(&conn, 1, fp, false).is_ok(), "unchanged");
        for change in [
            "UPDATE servers SET host = 'evil' WHERE id = 1",
            "UPDATE servers SET auth_type = 'vault', credential_id = 7 WHERE id = 1",
            "UPDATE servers SET jump_host_id = 3 WHERE id = 1",
            "UPDATE servers SET username = 'admin' WHERE id = 1",
            "UPDATE servers SET deleted = 1 WHERE id = 1",
        ] {
            conn.execute(change, []).unwrap();
            assert!(may_save_now(&conn, 1, fp, false).is_err(), "{change}");
            conn.execute("UPDATE servers SET host = 'db-01', port = 22, username = 'root', auth_type = 'custom_pass', credential_id = NULL, jump_host_id = NULL, deleted = 0 WHERE id = 1", []).unwrap();
            assert!(may_save_now(&conn, 1, fp, false).is_ok(), "back as it was after {change}");
        }
        // A VNC server linked to a saved login since: refused, however the
        // fingerprint came about.
        let vfp = save_fingerprint(&conn, 2).unwrap();
        assert!(may_save_now(&conn, 2, vfp, true).is_ok());
        conn.execute("UPDATE servers SET auth_type = 'vault', credential_id = 7 WHERE id = 2", []).unwrap();
        let linked = save_fingerprint(&conn, 2).unwrap();
        assert!(may_save_now(&conn, 2, linked, true).is_err());
        assert!(save_fingerprint(&conn, 99).is_none());
    }

    #[test]
    fn only_the_first_password_question_is_offered_for_saving() {
        let mut asked = false;
        // A code before the password: nothing to offer, nothing asked yet.
        assert_eq!(kbi_round(false, &mut asked), (false, false));
        // The password: offered.
        assert_eq!(kbi_round(true, &mut asked), (true, false));
        // A code after it: the password's answer stays.
        assert_eq!(kbi_round(false, &mut asked), (false, false));
        // A second password question: not offered, and the first answer goes.
        assert_eq!(kbi_round(true, &mut asked), (false, true));
        assert_eq!(kbi_round(true, &mut asked), (false, true));
    }

    #[test]
    fn a_password_is_saved_only_for_the_saved_login_name() {
        assert!(may_save_for_login(false, "root"));
        assert!(!may_save_for_login(true, "root"), "typed at Login as");
        assert!(!may_save_for_login(false, ""), "no saved name: typed earlier in the tab, or root by default");
        assert!(!may_save_for_login(false, "  "));
    }

    #[test]
    fn a_shared_profile_is_told_apart() {
        let conn = db();
        assert!(!saving_reaches_others(&conn));
        conn.execute("INSERT INTO sync_meta VALUES ('share_id', 'abc')", []).unwrap();
        assert!(saving_reaches_others(&conn));
    }

    /// The app's own tables, with a saved login used by two SSH servers, an
    /// SSH server with its own password, an RDP server with its own password
    /// and one with the saved login, and a VNC server.
    fn app_db() -> Connection {
        let (conn, _) = crate::sync_engine_tests::device("a");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO credentials(id, name, auth_type, username, password) VALUES (7, 'ops', 'custom_pass', 'ops', 'login-pw');
             INSERT INTO servers(id, name, host, port, username, password, credential_id, auth_type, protocol)
                 VALUES (1, 'web-01', 'web-01', 22, NULL, NULL, 7, 'vault', 'ssh'),
                        (2, 'web-02', 'web-02', 22, NULL, NULL, 7, 'vault', 'ssh'),
                        (3, 'db-01', 'db-01', 22, 'root', 'old-pw', NULL, 'custom_pass', 'ssh'),
                        (4, 'win-01', 'win-01', 3389, 'admin', 'rdp-pw', NULL, 'custom_pass', 'rdp'),
                        (5, 'win-02', 'win-02', 3389, NULL, NULL, 7, 'vault', 'rdp'),
                        (6, 'vnc-01', 'vnc-01', 5900, NULL, 'vnc-pw', NULL, 'custom_pass', 'vnc');",
        )
        .unwrap();
        conn
    }

    fn target(conn: &Connection, server_id: i32, vnc: bool) -> crate::SaveTarget {
        let fingerprint = save_fingerprint(conn, server_id).expect("the app's servers table has every column the fingerprint reads");
        crate::SaveTarget { server_id, generation: 0, ticked: true, keys: false, shared: false, fingerprint, vnc }
    }

    fn login_pw(conn: &Connection) -> String {
        conn.query_row("SELECT password FROM credentials WHERE id = 7", [], |r| r.get(0)).unwrap()
    }

    fn server_pw(conn: &Connection, id: i32) -> Option<String> {
        conn.query_row("SELECT password FROM servers WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn in_the_apps_tables_a_typed_password_goes_where_it_was_typed_for() {
        let conn = app_db();
        // Unchanged: the server's own password is replaced.
        let to = target(&conn, 3, false);
        assert_eq!(crate::save_typed_password(&conn, &to, "new-pw").unwrap(), "this server");
        assert_eq!(server_pw(&conn, 3).as_deref(), Some("new-pw"));
        // Linked to the shared saved login while the user typed: nothing is
        // saved, and the login keeps its password.
        let to = target(&conn, 3, false);
        conn.execute("UPDATE servers SET auth_type = 'vault', credential_id = 7 WHERE id = 3", []).unwrap();
        assert!(crate::save_typed_password(&conn, &to, "typed-for-db").is_err());
        assert_eq!(login_pw(&conn), "login-pw");
        // Pointed at another host while the user typed: nothing is saved.
        let to = target(&conn, 1, false);
        conn.execute("UPDATE servers SET host = 'elsewhere' WHERE id = 1", []).unwrap();
        assert!(crate::save_typed_password(&conn, &to, "typed-for-web").is_err());
        assert_eq!(login_pw(&conn), "login-pw");
        // Unchanged and using the saved login: the login's password, which the
        // prompt said.
        let to = target(&conn, 2, false);
        assert!(crate::save_typed_password(&conn, &to, "new-login-pw").unwrap().contains("ops"));
        assert_eq!(login_pw(&conn), "new-login-pw");
    }

    #[test]
    fn in_the_apps_tables_a_vnc_password_never_reaches_a_saved_login() {
        let conn = app_db();
        let to = target(&conn, 6, true);
        assert_eq!(crate::save_typed_password(&conn, &to, "vnc-new").unwrap(), "this server");
        assert_eq!(server_pw(&conn, 6).as_deref(), Some("vnc-new"));
        // Linked to the saved login since: refused, even with a fingerprint
        // taken after the link.
        conn.execute("UPDATE servers SET auth_type = 'vault', credential_id = 7 WHERE id = 6", []).unwrap();
        assert!(!vnc_may_save(&conn, 6));
        let to = target(&conn, 6, true);
        assert!(crate::save_typed_password(&conn, &to, "vnc-pw-2").is_err());
        assert_eq!(login_pw(&conn), "login-pw");
    }

    #[test]
    fn in_the_apps_tables_a_shared_profile_starts_the_tick_off() {
        let conn = app_db();
        assert!(!saving_reaches_others(&conn));
        conn.execute("INSERT INTO sync_meta(key, value) VALUES ('share_id', 'team')", []).unwrap();
        assert!(saving_reaches_others(&conn));
    }

    /// The hooks in the original project's code that call these rules, each
    /// where it must be. A sync that loses one in a merge fails here, naming
    /// it, instead of quietly dropping the rule (FORK.md, "The fork's own
    /// rules").
    /// E5: a personal profile may open by itself; once it's shared (or
    /// joined from a share), it may not.
    #[test]
    fn a_shared_profile_never_opens_by_itself() {
        let (conn, _h) = crate::sync_engine_tests::device("a");
        assert!(may_open_by_itself(&conn), "a personal profile");
        conn.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
        assert!(!may_open_by_itself(&conn), "a shared one");
        assert!(SHARED_AUTO_OPEN.starts_with("[QUICK_UNLOCK] SHARED:"), "the card shows what follows the tag");
    }

    #[test]
    fn the_hooks_are_in_place() {
        let lib = include_str!("lib.rs");
        for (hook, times, place) in [
            ("SaveTarget::for_vnc_server(", 1, "vnc_open's save target (E3)"),
            ("secret_policy::saving_reaches_others(", 1, "SaveTarget::for_server (E4)"),
            ("secret_policy::SHARED_PROFILE_NOTE", 1, "SaveTarget::offer (E4)"),
            ("secret_policy::save_fingerprint(", 1, "SaveTarget::for_server"),
            ("secret_policy::may_save_now(", 1, "save_typed_password"),
            ("save_typed_password(conn, &to, password)", 1, "save_prompted_password"),
            ("secret_policy::kbi_round(", 1, "run_keyboard_interactive"),
            ("secret_policy::may_save_for_login(", 2, "login_hop and initiate_connection"),
            ("secret_policy::stop_opening_by_itself(", 1, "share_current_profile (E5)"),
        ] {
            assert_eq!(lib.matches(hook).count(), times, "lib.rs should call `{hook}` {times}x, in {place}");
        }
        let quick_unlock = include_str!("quick_unlock.rs");
        for (hook, times, place) in [
            ("secret_policy::may_open_by_itself", 2, "quick_unlock_enable and quick_unlock (E5)"),
            ("secret_policy::SHARED_AUTO_OPEN", 1, "quick_unlock_enable (E5)"),
        ] {
            assert_eq!(quick_unlock.matches(hook).count(), times, "quick_unlock.rs should use `{hook}` {times}x, in {place}");
        }
    }
}

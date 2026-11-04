//! A server's forwards as a sync changed them, kept apart from a
//! session's own so a save doesn't undo the sync.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use synced_tunnels::*`.

use crate::*;

/// A server's saved `tunnels` JSON, by uuid. None when there's no such row.
pub(crate) fn stored_tunnels(conn: &Connection, uuid: &str) -> Option<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT tunnels FROM servers WHERE uuid = ?1", [uuid], |r| r.get::<_, Option<String>>(0))
        .optional()
        .ok()
        .flatten()
}

/// After the merge engine applied a server record: when it changed the
/// server's saved forwards, what they were before is kept (the first time
/// only, until a session takes it), in a table that lives as long as the
/// connection and is never saved or synced. A session's rule list read before
/// the sync is out of date then; `refresh_synced_session_rules` and
/// `persist_session_tunnels` use it so saving the session's list doesn't undo
/// what sync brought (a forward added elsewhere deleted everywhere, and its
/// hold let go).
pub(crate) fn note_synced_tunnels(conn: &Connection, uuid: &str, before: &Option<String>) -> Result<(), String> {
    if stored_tunnels(conn, uuid).as_ref() == Some(before) {
        return Ok(());
    }
    conn.execute_batch("CREATE TEMP TABLE IF NOT EXISTS synced_tunnels_base (uuid TEXT PRIMARY KEY, tunnels TEXT)")
        .map_err(|e| format!("[SYNC] TUNNELS_BASE: {e}"))?;
    conn.execute(
        "INSERT OR IGNORE INTO temp.synced_tunnels_base (uuid, tunnels) VALUES (?1, ?2)",
        rusqlite::params![uuid, before],
    )
    .map(|_| ())
    .map_err(|e| format!("[SYNC] TUNNELS_BASE: {e}"))
}

/// Takes what a server's saved forwards were before sync changed them (see
/// `note_synced_tunnels`): Some when they changed since a session last read
/// or saved them.
pub(crate) fn take_synced_tunnels_base(conn: &Connection, server_id: i32) -> Option<Vec<tunnel::TunnelSpec>> {
    let uuid = approvals::server_uuid(conn, server_id as i64)?;
    let before: Option<String> = conn
        .query_row("SELECT tunnels FROM temp.synced_tunnels_base WHERE uuid = ?1", [&uuid], |r| r.get(0))
        .ok()?;
    let _ = conn.execute("DELETE FROM temp.synced_tunnels_base WHERE uuid = ?1", [&uuid]);
    Some(before.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
}

/// A session whose server's saved forwards changed by sync since it read them
/// starts again from the saved ones (with this device's holds), before the
/// user sees or changes its rules. The session saves after every change, so
/// it has nothing of its own that isn't saved.
pub(crate) async fn refresh_synced_session_rules(state: &SshState, db_state: &DbState, session_id: &str, server_id: Option<i32>) -> Result<(), String> {
    let server_id = server_id.unwrap_or(0);
    if server_id <= 0 {
        return Ok(());
    }
    let stale = {
        let conn_guard = db_state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        take_synced_tunnels_base(conn, server_id).is_some()
    };
    if stale {
        let saved = saved_tunnel_rules(db_state, server_id)?;
        if let Some(rules) = state.session_tunnel_specs.lock().await.get_mut(session_id) {
            *rules = saved;
        }
    }
    Ok(())
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod synced_tunnels;", 1),
                ("", "use synced_tunnels::*;", 1),
                ("merge_batch", "stored_tunnels(", 1),
                ("merge_batch", "note_synced_tunnels(", 1),
                ("persist_session_tunnels", "take_synced_tunnels_base(", 1),
                ("ensure_session_rules", "refresh_synced_session_rules(", 1),
                ("initiate_connection", "refresh_synced_session_rules(", 1),
                ("list_tunnel_rules", "refresh_synced_session_rules(", 1),
                ("restart_session_tunnels", "refresh_synced_session_rules(", 1),
            ],
        );
    }
}

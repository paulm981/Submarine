//! A server whose jump host was deleted (#51) is never connected directly
//! without asking.
//!
//! The synced link (`servers.jump_host_id`) is still cleared when a jump host
//! goes, on the device that deletes it (tree_moves::unlink_jump_hosts) and on
//! every other one as the deletion arrives (lib.rs, apply_tombstone): a link to
//! a row that's gone would follow the server into merges and exports. What
//! keeps it from quietly going direct is a note on this device, in a table of
//! its own that never syncs: the servers that lost their jump host, with its
//! name. While a server has one, a terminal, SFTP, a Wall, Monitoring and Ping
//! all refuse it before any network connection, with a message saying why,
//! and the tab offers **Connect directly** and **Pick a jump host**.
//!
//! The note counts only while the server has no jump host: one picked since
//! (by its form, or by a sync that brought one) is used as normal. Saving the
//! server's form clears it, and so does **Connect directly**.

use rusqlite::{params, Connection, OptionalExtension};

/// The start of the error a refused connection returns: the page shows the
/// two choices for it.
pub(crate) const CODE: &str = "[JUMP_GONE]";

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS jump_hosts_gone (server_id INTEGER PRIMARY KEY, jump_name TEXT NOT NULL DEFAULT '')";

fn db(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| format!("[DATABASE] {}: {}", code, e)
}

/// Made when first needed, so every vault (and every test's) has it.
fn ensure(conn: &Connection) -> Result<(), String> {
    conn.execute(SCHEMA, []).map(|_| ()).map_err(db("JUMP_GONE_TABLE_FAILED"))
}

/// Before jump host `jump_id` is deleted here: each server that goes through
/// it is noted, with its name. (A deletion from another device is noted by
/// `note_lost`.)
pub(crate) fn note_behind(conn: &Connection, jump_id: i64) -> Result<(), String> {
    ensure(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO jump_hosts_gone(server_id, jump_name)
         SELECT s.id, COALESCE((SELECT name FROM servers WHERE id = ?1), '')
         FROM servers s WHERE s.jump_host_id = ?1 AND s.id <> ?1",
        params![jump_id],
    )
    .map(|_| ())
    .map_err(db("JUMP_GONE_NOTE_FAILED"))
}

/// A server (by its sync id) that came in from another device with a jump
/// host this device doesn't have, so its link was left empty. Its name isn't
/// known here. A note already made, with the name, stays.
pub(crate) fn note_unresolved(conn: &Connection, server_uuid: &str) -> Result<(), String> {
    ensure(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO jump_hosts_gone(server_id, jump_name) SELECT id, '' FROM servers WHERE uuid = ?1",
        params![server_uuid],
    )
    .map(|_| ())
    .map_err(db("JUMP_GONE_NOTE_FAILED"))
}

/// Each server's jump host before a sync merge: the server, the jump host
/// and its name. See `note_lost`.
pub(crate) fn links_before(conn: &Connection) -> Vec<(i64, i64, String)> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT s.id, s.jump_host_id, COALESCE(j.name, '') FROM servers s JOIN servers j ON j.id = s.jump_host_id WHERE s.id <> j.id",
    ) else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

/// After a sync merge: each server whose jump host the merge deleted is
/// noted, with its name, whatever order the records came in (a server's own
/// record can clear its link before the jump host's deletion is applied).
pub(crate) fn note_lost(conn: &Connection, before: &[(i64, i64, String)]) -> Result<(), String> {
    if before.is_empty() {
        return Ok(());
    }
    ensure(conn)?;
    let mut stmt = conn
        .prepare(
            "INSERT OR REPLACE INTO jump_hosts_gone(server_id, jump_name)
             SELECT ?1, ?3 WHERE EXISTS(SELECT 1 FROM servers WHERE id = ?1) AND NOT EXISTS(SELECT 1 FROM servers WHERE id = ?2)",
        )
        .map_err(db("JUMP_GONE_NOTE_FAILED"))?;
    for (server, jump, name) in before {
        stmt.execute(params![server, jump, name]).map_err(db("JUMP_GONE_NOTE_FAILED"))?;
    }
    Ok(())
}

/// The note goes: the server's form was saved, or the user chose to connect
/// directly.
pub(crate) fn clear(conn: &Connection, server_id: i64) -> Result<(), String> {
    ensure(conn)?;
    conn.execute("DELETE FROM jump_hosts_gone WHERE server_id = ?1", params![server_id])
        .map(|_| ())
        .map_err(db("JUMP_GONE_CLEAR_FAILED"))
}

/// What's said when such a server would connect.
pub(crate) fn message(server: &str, jump: &str) -> String {
    let server = if server.trim().is_empty() { "This server" } else { server.trim() };
    let jump = jump.trim();
    let whose = if jump.is_empty() { format!("{server}'s jump host") } else { format!("{server}'s jump host ({jump})") };
    format!("{whose} was deleted, so Submarine won't connect it directly without asking.")
}

/// Why `server_id` mustn't connect now, if it mustn't: its jump host was
/// deleted (the note), or its link points at a server that isn't there (a
/// vault from before the note). `None` when it has a jump host, or never lost
/// one.
pub(crate) fn reason(conn: &Connection, server_id: i64) -> Option<String> {
    let (name, jump_id, jump_exists): (Option<String>, Option<i64>, bool) = conn
        .query_row(
            "SELECT s.name, s.jump_host_id, EXISTS(SELECT 1 FROM servers j WHERE j.id = s.jump_host_id) FROM servers s WHERE s.id = ?1",
            params![server_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten()?;
    let name = name.unwrap_or_default();
    if jump_id.is_some() && jump_exists {
        return None;
    }
    let noted: Option<String> = conn
        .query_row("SELECT jump_name FROM jump_hosts_gone WHERE server_id = ?1", params![server_id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    match (jump_id, noted) {
        (_, Some(jump)) => Some(message(&name, &jump)),
        (Some(_), None) => Some(message(&name, "")),
        (None, None) => None,
    }
}

/// `Err` (starting with CODE) when `server_id` mustn't connect now.
pub(crate) fn refuse(conn: &Connection, server_id: i64) -> Result<(), String> {
    match reason(conn, server_id) {
        Some(why) => Err(format!("{CODE} {why}")),
        None => Ok(()),
    }
}

/// **Connect directly**, from a refused connection's choices: the note goes,
/// and a link left pointing at nothing is cleared, as an edit of the server
/// that syncs like any other. The host key is checked as usual when it
/// connects.
#[tauri::command]
pub async fn jump_host_connect_directly(state: tauri::State<'_, crate::DbState>, server_id: i32) -> Result<(), String> {
    {
        let guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        connect_directly(conn, i64::from(server_id))?;
    }
    crate::save_vault_internal(&state)
}

fn connect_directly(conn: &Connection, server_id: i64) -> Result<(), String> {
    clear(conn, server_id)?;
    conn.execute(
        "UPDATE servers SET jump_host_id = NULL
         WHERE id = ?1 AND jump_host_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM servers j WHERE j.id = servers.jump_host_id)",
        params![server_id],
    )
    .map(|_| ())
    .map_err(db("JUMP_GONE_DIRECT_FAILED"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, jump_host_id INTEGER, uuid TEXT);
             INSERT INTO servers VALUES (1, 'web-gw', NULL, 'u1'), (2, 'db-01', 1, 'u2'), (3, 'web-01', 1, 'u3'), (4, 'tool', NULL, 'u4');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_server_whose_jump_host_was_deleted_is_refused_until_asked() {
        let conn = vault();
        note_behind(&conn, 1).unwrap();
        // What deleting it does to the synced link.
        conn.execute_batch("UPDATE servers SET jump_host_id = NULL WHERE jump_host_id = 1; DELETE FROM servers WHERE id = 1;").unwrap();
        let err = refuse(&conn, 2).unwrap_err();
        assert_eq!(err, "[JUMP_GONE] db-01's jump host (web-gw) was deleted, so Submarine won't connect it directly without asking.");
        assert!(refuse(&conn, 3).is_err());
        assert!(refuse(&conn, 4).is_ok(), "never had one");
        // Connect directly: asked, so it may.
        connect_directly(&conn, 2).unwrap();
        assert!(refuse(&conn, 2).is_ok());
        // A jump host picked since counts, whatever the note says.
        conn.execute("UPDATE servers SET jump_host_id = 4 WHERE id = 3", []).unwrap();
        assert!(refuse(&conn, 3).is_ok());
    }

    #[test]
    fn a_link_to_a_server_that_isnt_there_is_refused_too() {
        let conn = vault();
        conn.execute("UPDATE servers SET jump_host_id = 99 WHERE id = 4", []).unwrap();
        assert_eq!(refuse(&conn, 4).unwrap_err(), "[JUMP_GONE] tool's jump host was deleted, so Submarine won't connect it directly without asking.");
        connect_directly(&conn, 4).unwrap();
        let jump: Option<i64> = conn.query_row("SELECT jump_host_id FROM servers WHERE id = 4", [], |r| r.get(0)).unwrap();
        assert_eq!(jump, None, "the dangling link is cleared");
        assert!(refuse(&conn, 4).is_ok());
    }

    #[test]
    fn one_that_came_in_from_another_device_without_its_jump_host_is_refused() {
        let conn = vault();
        conn.execute("UPDATE servers SET jump_host_id = NULL WHERE id = 3", []).unwrap();
        note_unresolved(&conn, "u3").unwrap();
        assert_eq!(refuse(&conn, 3).unwrap_err(), "[JUMP_GONE] web-01's jump host was deleted, so Submarine won't connect it directly without asking.");
        // A note with the name isn't replaced by one without.
        note_behind(&conn, 1).unwrap();
        note_unresolved(&conn, "u2").unwrap();
        conn.execute("UPDATE servers SET jump_host_id = NULL WHERE id = 2", []).unwrap();
        assert!(refuse(&conn, 2).unwrap_err().contains("(web-gw)"));
        clear(&conn, 2).unwrap();
        assert!(refuse(&conn, 2).is_ok());
    }
}

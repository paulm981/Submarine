//! Moving things around the server tree — the session tree's drag and drop
//! and its "Move to…": servers, Remote Desktop links and folders into another
//! folder, or to the top level (`None`).
//!
//! Each command is one statement per row and one vault save. Rows already
//! where they're going are left alone, so a no-op move doesn't stamp them as
//! edits for sync to push.

use crate::DbState;
use rusqlite::{params, Connection, OptionalExtension};

fn db(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| format!("[DATABASE] {}: {}", code, e)
}

/// The destination must be a folder that exists (or the top level).
fn check_folder(conn: &Connection, folder_id: Option<i64>) -> Result<(), String> {
    let Some(id) = folder_id else { return Ok(()) };
    let found: Option<i64> = conn
        .query_row("SELECT id FROM folders WHERE id=?1", params![id], |r| r.get(0))
        .optional()
        .map_err(db("FOLDER_QUERY_FAILED"))?;
    found.map(|_| ()).ok_or_else(|| "[VALIDATION] FOLDER_NOT_FOUND: that folder no longer exists".into())
}

/// Servers into `folder_id`, each at the bottom of its new folder's manual
/// order, as edit_server places a server it moves. Returns how many moved.
pub(crate) fn move_servers_in(conn: &Connection, ids: &[i64], folder_id: Option<i64>) -> Result<usize, String> {
    check_folder(conn, folder_id)?;
    let mut moved = 0;
    for id in ids {
        moved += conn
            .execute(
                "UPDATE servers SET folder_id = ?2,
                   position = COALESCE((SELECT MAX(position) FROM servers WHERE id != ?1 AND folder_id IS ?2), -1) + 1
                 WHERE id = ?1 AND folder_id IS NOT ?2",
                params![id, folder_id],
            )
            .map_err(db("SERVER_MOVE_FAILED"))?;
    }
    Ok(moved)
}

pub(crate) fn move_rdp_links_in(conn: &Connection, ids: &[i64], folder_id: Option<i64>) -> Result<usize, String> {
    check_folder(conn, folder_id)?;
    let mut moved = 0;
    for id in ids {
        moved += conn
            .execute("UPDATE rdp_links SET folder_id = ?2 WHERE id = ?1 AND folder_id IS NOT ?2", params![id, folder_id])
            .map_err(db("RDP_MOVE_FAILED"))?;
    }
    Ok(moved)
}

/// A folder into another (or to the top level) with everything in it. Never
/// into itself or a folder inside it: that would cut the branch off the tree.
pub(crate) fn move_folder_in(conn: &Connection, id: i64, parent_id: Option<i64>) -> Result<bool, String> {
    check_folder(conn, Some(id))?;
    check_folder(conn, parent_id)?;
    if let Some(p) = parent_id {
        let inside = crate::folder_subtree(conn, id).map_err(db("FOLDER_TREE_QUERY_FAILED"))?;
        if inside.contains(&p) {
            return Err("[VALIDATION] FOLDER_INTO_ITSELF: a folder can't go inside itself".into());
        }
    }
    let moved = conn
        .execute("UPDATE folders SET parent_id = ?2 WHERE id = ?1 AND parent_id IS NOT ?2", params![id, parent_id])
        .map_err(db("FOLDER_MOVE_FAILED"))?;
    Ok(moved > 0)
}

/// Run `f` on the open vault and save it when anything changed.
fn with_vault<T>(state: &tauri::State<'_, DbState>, f: impl FnOnce(&Connection) -> Result<(T, bool), String>) -> Result<T, String> {
    let (out, changed) = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        f(conn)?
    };
    if changed {
        crate::save_vault_internal(state)?;
    }
    Ok(out)
}

#[tauri::command]
pub async fn move_servers(state: tauri::State<'_, DbState>, ids: Vec<i64>, folder_id: Option<i64>) -> Result<(), String> {
    with_vault(&state, |conn| move_servers_in(conn, &ids, folder_id).map(|n| ((), n > 0)))
}

#[tauri::command]
pub async fn move_rdp_links(state: tauri::State<'_, DbState>, ids: Vec<i64>, folder_id: Option<i64>) -> Result<(), String> {
    with_vault(&state, |conn| move_rdp_links_in(conn, &ids, folder_id).map(|n| ((), n > 0)))
}

#[tauri::command]
pub async fn move_folder(state: tauri::State<'_, DbState>, id: i64, parent_id: Option<i64>) -> Result<(), String> {
    with_vault(&state, |conn| move_folder_in(conn, id, parent_id).map(|moved| ((), moved)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE folders(id INTEGER PRIMARY KEY, name TEXT, parent_id INTEGER);
             CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, folder_id INTEGER, position INTEGER NOT NULL DEFAULT 0);
             INSERT INTO folders VALUES (1, 'Prod', NULL), (2, 'East', 1), (3, 'Lab', NULL);
             INSERT INTO servers VALUES (10, 'web1', 1, 0), (11, 'web2', 1, 1), (12, 'db1', 2, 0), (13, 'tool', NULL, 0);",
        )
        .unwrap();
        conn.execute_batch(crate::rdp::SCHEMA).unwrap();
        conn.execute("INSERT INTO rdp_links (id, name, host, port, username, folder_id, notes) VALUES (20, 'desk', 'desk.example', 3389, '', 1, '')", [])
            .unwrap();
        conn
    }

    fn folder_of(conn: &Connection, table: &str, id: i64) -> Option<i64> {
        conn.query_row(&format!("SELECT folder_id FROM {} WHERE id=?1", table), [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn servers_move_to_the_bottom_of_their_new_folder() {
        let conn = vault();
        assert_eq!(move_servers_in(&conn, &[10, 13], Some(2)).unwrap(), 2);
        assert_eq!(folder_of(&conn, "servers", 10), Some(2));
        let positions: Vec<(i64, i64)> = conn
            .prepare("SELECT id, position FROM servers WHERE folder_id=2 ORDER BY position")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(positions, vec![(12, 0), (10, 1), (13, 2)]);
        // Already there, or to the top level.
        assert_eq!(move_servers_in(&conn, &[12], Some(2)).unwrap(), 0);
        assert_eq!(move_servers_in(&conn, &[12], None).unwrap(), 1);
        assert_eq!(folder_of(&conn, "servers", 12), None);
        assert!(move_servers_in(&conn, &[11], Some(99)).is_err(), "no such folder");
    }

    #[test]
    fn links_move_like_servers() {
        let conn = vault();
        assert_eq!(move_rdp_links_in(&conn, &[20], Some(3)).unwrap(), 1);
        assert_eq!(folder_of(&conn, "rdp_links", 20), Some(3));
        assert_eq!(move_rdp_links_in(&conn, &[20], None).unwrap(), 1);
        assert_eq!(folder_of(&conn, "rdp_links", 20), None);
    }

    #[test]
    fn a_folder_moves_with_what_is_in_it_but_never_into_itself() {
        let conn = vault();
        assert!(move_folder_in(&conn, 3, Some(1)).unwrap());
        let parent: Option<i64> = conn.query_row("SELECT parent_id FROM folders WHERE id=3", [], |r| r.get(0)).unwrap();
        assert_eq!(parent, Some(1));
        assert!(move_folder_in(&conn, 1, Some(1)).is_err(), "into itself");
        assert!(move_folder_in(&conn, 1, Some(2)).is_err(), "into its own subfolder");
        assert!(move_folder_in(&conn, 2, None).unwrap(), "out to the top level");
        assert!(!move_folder_in(&conn, 2, None).unwrap(), "already there");
        assert_eq!(folder_of(&conn, "servers", 12), Some(2), "its servers stay in it");
    }
}

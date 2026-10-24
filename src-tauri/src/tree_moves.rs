//! Changing the server tree from the session tree — its drag and drop, "Move
//! to…" and Delete: servers, Remote Desktop links and folders into another
//! folder (or the top level, `None`), or out of the vault.
//!
//! Each command is one transaction and one vault save, however many rows it
//! touches. Rows already where they're going are left alone, so a no-op
//! move doesn't stamp them as edits for sync to push.

use crate::{DbState, MonitorMap};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

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

/// Sync is last-write-wins per row, so two devices moving folders into each
/// other merge into a loop (X inside Y, Y inside X). The page shows every
/// folder of a loop at the top level (src/util/folderPath.ts); this makes it
/// so in the vault too — as an edit, which sync carries to the other devices
/// — so deleting one of them can't take the other with it. Run after every
/// sync merge, and before a folder is moved or deleted. Returns how many
/// folders it moved out.
pub(crate) fn break_folder_loops(conn: &Connection) -> rusqlite::Result<usize> {
    let parents: HashMap<i64, Option<i64>> = conn
        .prepare("SELECT id, parent_id FROM folders")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut in_loop = Vec::new();
    for &id in parents.keys() {
        // Walking up from `id` comes back to it: `id` is in a loop. A loop
        // further up is left to its own members, which each find it.
        let mut seen = HashSet::new();
        let mut cur = parents.get(&id).copied().flatten();
        while let Some(p) = cur {
            if p == id {
                in_loop.push(id);
                break;
            }
            if !seen.insert(p) {
                break;
            }
            cur = parents.get(&p).copied().flatten();
        }
    }
    for id in &in_loop {
        conn.execute("UPDATE folders SET parent_id = NULL WHERE id = ?1", params![id])?;
    }
    Ok(in_loop.len())
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

/// Remote Desktop links into `folder_id`. Returns how many moved.
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
    break_folder_loops(conn).map_err(db("FOLDER_LOOP_REPAIR_FAILED"))?;
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

/// Server, link and folder ids, in that order.
type Scope = (Vec<i64>, Vec<i64>, Vec<i64>);

/// Everything a delete takes: the folders given and every folder inside
/// them, the servers and links given, and every server and link in those
/// folders. Loops are broken first, so "inside" means what the page shows.
pub(crate) fn delete_scope(conn: &Connection, servers: &[i64], links: &[i64], folders: &[i64]) -> Result<Scope, String> {
    break_folder_loops(conn).map_err(db("FOLDER_LOOP_REPAIR_FAILED"))?;
    let mut all_folders: Vec<i64> = Vec::new();
    for f in folders {
        for id in crate::folder_subtree(conn, *f).map_err(db("FOLDER_TREE_QUERY_FAILED"))? {
            if !all_folders.contains(&id) {
                all_folders.push(id);
            }
        }
    }
    let in_folders = |table: &str| -> Result<Vec<i64>, String> {
        let mut stmt = conn.prepare(&format!("SELECT id FROM {} WHERE folder_id = ?1", table)).map_err(db("TREE_QUERY_FAILED"))?;
        let mut out = Vec::new();
        for f in &all_folders {
            let ids = stmt.query_map(params![f], |r| r.get::<_, i64>(0)).map_err(db("TREE_QUERY_FAILED"))?;
            for id in ids {
                out.push(id.map_err(db("TREE_QUERY_FAILED"))?);
            }
        }
        Ok(out)
    };
    let mut all_servers: Vec<i64> = servers.to_vec();
    all_servers.extend(in_folders("servers")?);
    all_servers.sort_unstable();
    all_servers.dedup();
    let mut all_links: Vec<i64> = links.to_vec();
    all_links.extend(in_folders("rdp_links")?);
    all_links.sort_unstable();
    all_links.dedup();
    Ok((all_servers, all_links, all_folders))
}

/// Before servers are deleted, every server that went through one of them as
/// its jump host is noted on this device (jump_gone.rs), so it's never
/// connected directly without asking, and its link is cleared rather than
/// left pointing at nothing, as when the deletion comes from another device
/// (lib.rs, apply_tombstone). Clearing it is a local repair, not an edit to
/// push: each device does it when the deletion reaches it.
pub(crate) fn unlink_jump_hosts(conn: &Connection, servers: &[i64]) -> Result<(), String> {
    if servers.is_empty() {
        return Ok(());
    }
    for id in servers {
        crate::jump_gone::note_behind(conn, *id)?;
    }
    crate::with_merge_flag(conn, |c| {
        let mut stmt =
            c.prepare("UPDATE servers SET jump_host_id = NULL WHERE jump_host_id = ?1").map_err(db("JUMP_UNLINK_FAILED"))?;
        for id in servers {
            stmt.execute(params![id]).map_err(db("JUMP_UNLINK_FAILED"))?;
        }
        Ok(())
    })
}

fn delete_in(conn: &Connection, servers: &[i64], links: &[i64], folders: &[i64]) -> Result<(), String> {
    unlink_jump_hosts(conn, servers)?;
    for id in servers {
        conn.execute("DELETE FROM servers WHERE id = ?1", params![id]).map_err(db("SERVER_DELETE_FAILED"))?;
    }
    for id in links {
        conn.execute("DELETE FROM rdp_links WHERE id = ?1", params![id]).map_err(db("RDP_DELETE_FAILED"))?;
    }
    for id in folders {
        conn.execute("DELETE FROM folders WHERE id = ?1", params![id]).map_err(db("FOLDER_DELETE_FAILED"))?;
    }
    Ok(())
}

/// Run `f` on the open vault in one transaction, and save the vault when
/// anything changed. A failure part way leaves nothing done.
fn with_vault<T>(state: &tauri::State<'_, DbState>, f: impl FnOnce(&Connection) -> Result<(T, bool), String>) -> Result<T, String> {
    let (out, changed) = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        let tx = conn.unchecked_transaction().map_err(db("TRANSACTION_FAILED"))?;
        let out = f(&tx)?;
        tx.commit().map_err(db("TRANSACTION_FAILED"))?;
        out
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

/// Delete servers, links and folders (with everything in the folders) in
/// one go: the session tree's Delete on a selection. Each server's monitor
/// is stopped first, as delete_server does, or it would keep polling a host
/// whose credentials are gone.
#[tauri::command]
pub async fn delete_tree_items(
    state: tauri::State<'_, DbState>,
    map: tauri::State<'_, MonitorMap>,
    servers: Vec<i64>,
    links: Vec<i64>,
    folders: Vec<i64>,
) -> Result<(), String> {
    let (servers, links, folders) = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        delete_scope(conn, &servers, &links, &folders)?
    };
    for id in &servers {
        crate::monitor::stop_monitor(map.inner().clone(), *id as i32).await;
    }
    with_vault(&state, |conn| delete_in(conn, &servers, &links, &folders).map(|_| ((), true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE folders(id INTEGER PRIMARY KEY, name TEXT, parent_id INTEGER);
             CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, folder_id INTEGER, position INTEGER NOT NULL DEFAULT 0, jump_host_id INTEGER);
             CREATE TABLE sync_flags(key TEXT PRIMARY KEY, val INTEGER NOT NULL);
             INSERT INTO folders VALUES (1, 'Prod', NULL), (2, 'East', 1), (3, 'Lab', NULL);
             INSERT INTO servers VALUES (10, 'web1', 1, 0, NULL), (11, 'web2', 1, 1, NULL), (12, 'db1', 2, 0, NULL), (13, 'tool', NULL, 0, NULL);",
        )
        .unwrap();
        conn.execute_batch(crate::rdp_links::SCHEMA).unwrap();
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
    fn a_loop_from_sync_is_broken_and_a_delete_stays_inside_what_it_names() {
        let conn = vault();
        // Prod (1) inside Lab (3), Lab inside Prod: what two devices' moves
        // can merge into.
        conn.execute_batch("UPDATE folders SET parent_id = 3 WHERE id = 1; UPDATE folders SET parent_id = 1 WHERE id = 3;").unwrap();
        let (servers, links, folders) = delete_scope(&conn, &[], &[], &[3]).unwrap();
        assert_eq!(folders, vec![3], "Lab alone: Prod is no longer inside it");
        assert!(servers.is_empty() && links.is_empty());
        let roots: Vec<i64> = conn.prepare("SELECT id FROM folders WHERE parent_id IS NULL ORDER BY id").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(roots, vec![1, 3]);
        // East (2) stays inside Prod.
        let (servers, links, folders) = delete_scope(&conn, &[13], &[], &[1]).unwrap();
        assert_eq!(folders, vec![1, 2]);
        assert_eq!(servers, vec![10, 11, 12, 13]);
        assert_eq!(links, vec![20]);
        delete_in(&conn, &servers, &links, &folders).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn deleting_a_jump_host_leaves_the_servers_behind_it_refusing_to_go_direct() {
        let conn = vault();
        // web1 and web2 go through tool; db1 through web1.
        conn.execute_batch(
            "UPDATE servers SET jump_host_id = 13 WHERE id IN (10, 11); UPDATE servers SET jump_host_id = 10 WHERE id = 12;
             CREATE TABLE stamped(id INTEGER);
             CREATE TRIGGER stamp AFTER UPDATE ON servers WHEN COALESCE((SELECT val FROM sync_flags WHERE key='merge'),0)=0
               BEGIN INSERT INTO stamped VALUES (NEW.id); END;",
        )
        .unwrap();
        delete_in(&conn, &[13], &[], &[]).unwrap();
        let jump = |id: i64| -> Option<i64> { conn.query_row("SELECT jump_host_id FROM servers WHERE id=?1", [id], |r| r.get(0)).unwrap() };
        assert_eq!((jump(10), jump(11)), (None, None), "unlinked, not left pointing at nothing");
        assert_eq!(jump(12), Some(10), "another jump host is left alone");
        // ...but noted, so neither connects directly without asking.
        let refused = crate::jump_gone::refuse(&conn, 10).unwrap_err();
        assert!(refused.starts_with("[JUMP_GONE] web1's jump host (tool) was deleted"), "{refused}");
        assert!(crate::jump_gone::refuse(&conn, 11).is_err());
        assert!(crate::jump_gone::refuse(&conn, 12).is_ok());
        let stamped: i64 = conn.query_row("SELECT COUNT(*) FROM stamped", [], |r| r.get(0)).unwrap();
        assert_eq!(stamped, 0, "a local repair, not edits for sync to push");
        // Deleting one behind it as well as it, in one go.
        delete_in(&conn, &[10, 12], &[], &[]).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 1);
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

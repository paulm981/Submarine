//! MultiExec — typing into many servers at once from a Wall (src/components/
//! WallView.tsx), and what it takes to open a hundred of them:
//!
//! - `write_terminals_data`: one keystroke (or paste) to every terminal of a
//!   broadcast in one call, instead of one IPC round-trip per server.
//! - A login queue per jump host: at most `limit` sessions log in to the same
//!   jump host at a time (default 8, 0 = no limit), the rest wait their turn.
//!   A bastion counts logins that haven't finished as "startups" and drops
//!   connections past its MaxStartups (OpenSSH's default starts refusing at
//!   10), so opening a folder of 100 servers behind one used to fail most of
//!   them. Servers reached directly aren't queued.
//! - Saved walls: a named set of servers to open as one Wall later. Kept in
//!   the vault's `walls` table, which is device-local like `rdp_links` — not
//!   cloud-synced, and travels with profile export/import.

use crate::ssh_manager::{ConnectAttempt, SshState, TerminalCommand};
use crate::DbState;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// ── Typing to many ─────────────────────────────────────────────────────────

/// The same bytes to each terminal. Terminals that are gone (closed, or their
/// session dropped) are skipped; the rest still get the data. Each terminal's
/// queue takes it straight away unless that terminal's link is backed up;
/// only those are waited on, after everyone else has theirs, so one slow
/// server doesn't hold up the keystroke for the other ninety-nine. (A full
/// queue doesn't let a later write in ahead of one waiting on it, so each
/// terminal still gets the keys in order.)
#[tauri::command]
pub async fn write_terminals_data(state: tauri::State<'_, SshState>, terminal_ids: Vec<String>, data: Vec<u8>) -> Result<(), String> {
    use tokio::sync::mpsc::error::TrySendError;
    // Senders are cloned out so the map isn't held across the sends (see
    // write_terminal_data).
    let txs: Vec<_> = {
        let map = state.terminal_txs.lock().await;
        let mut seen = HashSet::new();
        terminal_ids
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .filter_map(|id| map.get(id).cloned())
            .collect()
    };
    let mut backed_up = Vec::new();
    for tx in txs {
        if let Err(TrySendError::Full(cmd)) = tx.try_send(TerminalCommand::Data(data.clone())) {
            backed_up.push((tx, cmd));
        }
    }
    for (tx, cmd) in backed_up {
        let _ = tx.send(cmd).await;
    }
    Ok(())
}

// ── Login queue per jump host ──────────────────────────────────────────────

const LOGIN_LIMIT_FILE: &str = "jump_login_limit.json";
pub(crate) const DEFAULT_LOGIN_LIMIT: i64 = 8;
const MAX_LOGIN_LIMIT: i64 = 256;

/// -1 until read from the settings file.
static LOGIN_LIMIT: AtomicI64 = AtomicI64::new(-1);

/// Jump host id → (the limit it was made with, its slots). A changed limit
/// gets new slots; logins holding the old ones finish as they were.
type SlotMap = HashMap<i32, (i64, Arc<Semaphore>)>;
static LOGIN_SLOTS: Mutex<Option<SlotMap>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct LimitFile {
    limit: i64,
}

fn login_limit(app: &tauri::AppHandle) -> i64 {
    let cached = LOGIN_LIMIT.load(Ordering::Relaxed);
    if cached >= 0 {
        return cached;
    }
    let limit = crate::local_open::read_setting::<LimitFile>(app, LOGIN_LIMIT_FILE)
        .map(|f| f.limit.clamp(0, MAX_LOGIN_LIMIT))
        .unwrap_or(DEFAULT_LOGIN_LIMIT);
    LOGIN_LIMIT.store(limit, Ordering::Relaxed);
    limit
}

fn slots_for(jump_server_id: i32, limit: i64) -> Arc<Semaphore> {
    let mut guard = LOGIN_SLOTS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(&jump_server_id) {
        Some((made_with, sem)) if *made_with == limit => Arc::clone(sem),
        _ => {
            let sem = Arc::new(Semaphore::new(limit as usize));
            map.insert(jump_server_id, (limit, Arc::clone(&sem)));
            sem
        }
    }
}

/// A turn to log in to this jump host, held until dropped: right away while
/// fewer than the limit are logging in, otherwise once one of them is done —
/// first come, first served. `None` with no limit set. `waiting` is called
/// once if the turn doesn't come straight away (for the session's log).
/// Gives up when the attempt is replaced or its tab closed.
pub(crate) async fn login_turn(
    app: &tauri::AppHandle,
    jump_server_id: i32,
    attempt: &ConnectAttempt,
    waiting: impl FnOnce(i64),
) -> Result<Option<OwnedSemaphorePermit>, String> {
    let limit = login_limit(app);
    if limit == 0 {
        return Ok(None);
    }
    wait_for_turn(slots_for(jump_server_id, limit), attempt, || waiting(limit)).await.map(Some)
}

async fn wait_for_turn(sem: Arc<Semaphore>, attempt: &ConnectAttempt, waiting: impl FnOnce()) -> Result<OwnedSemaphorePermit, String> {
    if let Ok(permit) = Arc::clone(&sem).try_acquire_owned() {
        return Ok(permit);
    }
    waiting();
    // One acquire, polled until it completes, keeps this login's place in the
    // queue; the timer only checks whether it is still wanted.
    let acquire = sem.acquire_owned();
    tokio::pin!(acquire);
    loop {
        tokio::select! {
            permit = &mut acquire => {
                return permit.map_err(|_| "jump host login queue closed".to_string());
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                if !attempt.is_current().await {
                    return Err("stopped waiting for a jump host login slot".into());
                }
            }
        }
    }
}

#[tauri::command]
pub async fn get_jump_login_limit(app: tauri::AppHandle) -> Result<i64, String> {
    Ok(login_limit(&app))
}

#[tauri::command]
pub async fn set_jump_login_limit(app: tauri::AppHandle, limit: i64) -> Result<i64, String> {
    let limit = limit.clamp(0, MAX_LOGIN_LIMIT);
    crate::local_open::write_setting(&app, LOGIN_LIMIT_FILE, &LimitFile { limit }, "the jump host login limit")?;
    LOGIN_LIMIT.store(limit, Ordering::Relaxed);
    Ok(limit)
}

// ── Saved walls ────────────────────────────────────────────────────────────

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS walls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    server_ids TEXT NOT NULL DEFAULT '[]',
    cols INTEGER NOT NULL DEFAULT 0
)";

/// More than anyone would watch at once; keeps a bad row from opening
/// thousands of connections.
const MAX_WALL_SERVERS: usize = 500;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct SavedWall {
    #[serde(default)]
    pub id: Option<i64>,
    pub name: String,
    /// In tile order.
    #[serde(default)]
    pub server_ids: Vec<i64>,
    /// Tiles per row; 0 = fit to the window.
    #[serde(default)]
    pub cols: i64,
}

fn db(code: &'static str) -> impl Fn(rusqlite::Error) -> String {
    move |e| format!("[DATABASE] {}: {}", code, e)
}

/// Each wall with the servers it names that still exist (a deleted server
/// just drops out of it).
pub(crate) fn list(conn: &Connection) -> Result<Vec<SavedWall>, String> {
    let existing: HashSet<i64> = conn
        .prepare("SELECT id FROM servers")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(db("WALL_LIST_FAILED"))?;
    let mut stmt = conn
        .prepare("SELECT id, name, server_ids, cols FROM walls ORDER BY lower(name), id")
        .map_err(db("WALL_LIST_FAILED"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))
        .map_err(db("WALL_LIST_FAILED"))?;
    let mut walls = Vec::new();
    for row in rows {
        let (id, name, ids, cols) = row.map_err(db("WALL_LIST_FAILED"))?;
        let ids: Vec<i64> = serde_json::from_str(&ids).unwrap_or_default();
        walls.push(SavedWall {
            id: Some(id),
            name,
            server_ids: ids.into_iter().filter(|i| existing.contains(i)).collect(),
            cols: cols.clamp(0, 6),
        });
    }
    Ok(walls)
}

pub(crate) fn save(conn: &Connection, wall: &SavedWall) -> Result<i64, String> {
    let name = wall.name.trim();
    if name.is_empty() {
        return Err("Give the wall a name.".into());
    }
    if name.chars().count() > 200 {
        return Err("That name is too long.".into());
    }
    let mut seen = HashSet::new();
    let ids: Vec<i64> = wall.server_ids.iter().copied().filter(|i| seen.insert(*i)).collect();
    if ids.len() > MAX_WALL_SERVERS {
        return Err(format!("A wall holds up to {} servers.", MAX_WALL_SERVERS));
    }
    let ids = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    let cols = wall.cols.clamp(0, 6);
    match wall.id {
        Some(id) => {
            let n = conn
                .execute("UPDATE walls SET name=?1, server_ids=?2, cols=?3 WHERE id=?4", params![name, ids, cols, id])
                .map_err(db("WALL_SAVE_FAILED"))?;
            if n == 0 {
                return Err("That wall no longer exists.".into());
            }
            Ok(id)
        }
        None => {
            conn.execute("INSERT INTO walls (name, server_ids, cols) VALUES (?1, ?2, ?3)", params![name, ids, cols])
                .map_err(db("WALL_SAVE_FAILED"))?;
            Ok(conn.last_insert_rowid())
        }
    }
}

#[tauri::command]
pub async fn wall_list(state: tauri::State<'_, DbState>) -> Result<Vec<SavedWall>, String> {
    let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
    let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
    list(conn)
}

#[tauri::command]
pub async fn wall_save(state: tauri::State<'_, DbState>, wall: SavedWall) -> Result<i64, String> {
    let id = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        save(conn, &wall)?
    };
    crate::save_vault_internal(&state)?;
    Ok(id)
}

#[tauri::command]
pub async fn wall_delete(state: tauri::State<'_, DbState>, id: i64) -> Result<(), String> {
    {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        conn.execute("DELETE FROM walls WHERE id=?1", params![id]).map_err(db("WALL_DELETE_FAILED"))?;
    }
    crate::save_vault_internal(&state)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE servers (id INTEGER PRIMARY KEY, name TEXT);").unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        for id in [1, 2, 3] {
            conn.execute("INSERT INTO servers (id, name) VALUES (?1, 's')", params![id]).unwrap();
        }
        conn
    }

    #[test]
    fn walls_save_list_and_drop_deleted_servers() {
        let conn = conn();
        let mut w = SavedWall { id: None, name: " web ".into(), server_ids: vec![3, 1, 3, 2], cols: 9 };
        let id = save(&conn, &w).unwrap();
        w.id = Some(id);
        assert_eq!(
            list(&conn).unwrap(),
            vec![SavedWall { id: Some(id), name: "web".into(), server_ids: vec![3, 1, 2], cols: 6 }]
        );
        conn.execute("DELETE FROM servers WHERE id = 1", []).unwrap();
        assert_eq!(list(&conn).unwrap()[0].server_ids, vec![3, 2]);
        w.name = "renamed".into();
        w.server_ids = vec![2];
        save(&conn, &w).unwrap();
        assert_eq!(list(&conn).unwrap()[0].name, "renamed");
        assert!(save(&conn, &SavedWall { id: None, name: "  ".into(), server_ids: vec![], cols: 0 }).is_err());
        assert!(save(&conn, &SavedWall { id: Some(999), name: "gone".into(), server_ids: vec![], cols: 0 }).is_err());
    }

    #[test]
    fn slots_follow_the_limit() {
        let a = slots_for(-7, 2);
        assert!(Arc::ptr_eq(&a, &slots_for(-7, 2)));
        let p1 = Arc::clone(&a).try_acquire_owned().unwrap();
        let _p2 = Arc::clone(&a).try_acquire_owned().unwrap();
        assert!(Arc::clone(&a).try_acquire_owned().is_err());
        drop(p1);
        assert!(Arc::clone(&a).try_acquire_owned().is_ok());
        // A new limit, new slots; another jump host has its own.
        assert!(!Arc::ptr_eq(&a, &slots_for(-7, 3)));
        assert!(!Arc::ptr_eq(&slots_for(-7, 3), &slots_for(-8, 3)));
    }

    #[tokio::test]
    async fn a_login_waits_for_a_turn_and_gives_up_when_its_tab_moves_on() {
        let generations = Arc::new(tokio::sync::Mutex::new(HashMap::from([("s".to_string(), 1u64)])));
        let attempt = ConnectAttempt::new(Arc::clone(&generations), "s".into(), 1);
        let sem = Arc::new(Semaphore::new(1));
        let held = Arc::clone(&sem).try_acquire_owned().unwrap();

        // Waits (and says so) until the login ahead of it is done.
        let mut said = false;
        let turn = {
            let (sem, attempt) = (Arc::clone(&sem), attempt.clone());
            tokio::spawn(async move {
                let r = wait_for_turn(sem, &attempt, || said = true).await;
                (r.is_ok(), said)
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!turn.is_finished());
        drop(held);
        assert_eq!(turn.await.unwrap(), (true, true));

        // The tab reconnected or closed while waiting: it stops waiting.
        let _held = Arc::clone(&sem).try_acquire_owned().unwrap();
        generations.lock().await.insert("s".into(), 2);
        assert!(wait_for_turn(Arc::clone(&sem), &attempt, || {}).await.is_err());
    }
}

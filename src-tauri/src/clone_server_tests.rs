//! The fork's own tests, kept in a file of their own (#136) so a sync with
//! the original project doesn't conflict on them.

use super::*;

/// The servers columns the copy reads, a stand-in for the sync triggers
/// (a uuid on insert, a tombstone on delete), and held entries whose copy
/// can be made to fail.
fn db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, host TEXT, port INTEGER, username TEXT, password TEXT,
            credential_id INTEGER, folder_id INTEGER, proxy_type TEXT, proxy_host TEXT, proxy_port INTEGER, tunnels TEXT,
            auth_type TEXT, key_id INTEGER, autostart INTEGER NOT NULL DEFAULT 0, mirrors TEXT, color TEXT, notes TEXT,
            run_on_connect TEXT NOT NULL DEFAULT '', run_on_connect_every INTEGER, jump_host_id INTEGER, terminal_theme TEXT, highlight TEXT, protocol TEXT,
            position INTEGER NOT NULL DEFAULT 0, uuid TEXT);
         CREATE TABLE sync_tombstones(uuid TEXT PRIMARY KEY);
         CREATE TRIGGER s_ai AFTER INSERT ON servers WHEN NEW.uuid IS NULL
           BEGIN UPDATE servers SET uuid = 'u' || NEW.id WHERE id = NEW.id; END;
         CREATE TRIGGER s_ad AFTER DELETE ON servers BEGIN INSERT INTO sync_tombstones VALUES (OLD.uuid); END;
         CREATE TABLE held_approvals(uuid TEXT NOT NULL, item TEXT NOT NULL, since INTEGER NOT NULL, PRIMARY KEY (uuid, item));
         INSERT INTO servers(id, name, host, protocol) VALUES (1, 'web', 'example.com', 'vnc');
         INSERT INTO held_approvals VALUES ('u1', 'run_on_connect', 0);",
    )
    .unwrap();
    conn
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn copies_the_server_and_what_it_has_held() {
    let conn = db();
    let new_id = clone_server_row(&conn, 1).unwrap();
    let name: String = conn.query_row("SELECT name FROM servers WHERE id = ?1", [new_id], |r| r.get(0)).unwrap();
    assert_eq!(name, "web (copy)");
    // A copy is the same kind of server (#173).
    let protocol: Option<String> = conn.query_row("SELECT protocol FROM servers WHERE id = ?1", [new_id], |r| r.get(0)).unwrap();
    assert_eq!(protocol.as_deref(), Some("vnc"));
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM held_approvals"), 2);
    assert!(conn.is_autocommit());
}

#[test]
fn a_failed_copy_leaves_nothing_behind() {
    let conn = db();
    conn.execute_batch(
        "CREATE TRIGGER held_fails BEFORE INSERT ON held_approvals BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    )
    .unwrap();
    assert!(clone_server_row(&conn, 1).is_err());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM servers"), 1);
    // Taken back, not deleted: no tombstone for other devices to see.
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_tombstones"), 0);
    // And nothing is left open: the next change commits as usual.
    assert!(conn.is_autocommit());
}

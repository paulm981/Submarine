//! The fork's own tests, kept in a file of their own (#136) so a sync with
//! the original project doesn't conflict on them.

use super::*;

fn vault() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, host TEXT, port INTEGER, username TEXT, password TEXT,
            credential_id INTEGER, key_id INTEGER, auth_type TEXT, proxy_type TEXT, proxy_host TEXT, proxy_port INTEGER,
            jump_host_id INTEGER, protocol TEXT);
         CREATE TABLE credentials(id INTEGER PRIMARY KEY, username TEXT, password TEXT, key_id INTEGER, auth_type TEXT);
         CREATE TABLE sync_flags(key TEXT PRIMARY KEY, val INTEGER NOT NULL);
         INSERT INTO servers(id, name, host, port, username, auth_type) VALUES (1, 'bastion', 'b.example', 22, 'ops', 'custom');
         INSERT INTO servers(id, name, host, port, username, auth_type, jump_host_id) VALUES (2, 'web-01', 'w.example', 22, 'ops', 'custom', 1);
         INSERT INTO servers(id, name, host, port, username, auth_type, jump_host_id) VALUES (3, 'db-01', 'd.example', 22, 'ops', 'custom', 99);
         INSERT INTO servers(id, name, host, port, username, auth_type, jump_host_id) VALUES (4, 'db-02', 'e.example', 22, 'ops', 'custom', 5);
         INSERT INTO servers(id, name, host, port, username, auth_type) VALUES (5, 'web-gw', 'g.example', 22, 'ops', 'custom');",
    )
    .unwrap();
    conn
}

#[test]
fn a_server_whose_jump_host_is_gone_is_skipped_and_said_so() {
    let conn = vault();
    let web = resolve_node_auth_for_monitor(&conn, 2).unwrap();
    assert_eq!(web.jump.as_deref().map(|j| j.host.as_str()), Some("b.example"));
    let why = match resolve_node_auth_for_monitor(&conn, 3) {
        Ok(_) => panic!("db-01's jump host is gone"),
        Err(e) => e,
    };
    assert_eq!(
        resume_skipped_line("db-01", &why),
        "db-01's jump host was deleted, so Submarine won't connect it directly without asking. Skipped."
    );
    // Deleted here: noted with its name, the link cleared, and still skipped.
    crate::tree_moves::unlink_jump_hosts(&conn, &[5]).unwrap();
    conn.execute("DELETE FROM servers WHERE id = 5", []).unwrap();
    let why = match resolve_node_auth_for_monitor(&conn, 4) {
        Ok(_) => panic!("db-02's jump host was deleted"),
        Err(e) => e,
    };
    assert_eq!(
        resume_skipped_line("db-02", &why),
        "db-02's jump host (web-gw) was deleted, so Submarine won't connect it directly without asking. Skipped."
    );
    assert_eq!(resume_skipped_line("rdp-01", "an RDP server can't be monitored"), "rdp-01: an RDP server can't be monitored. Skipped.");
}

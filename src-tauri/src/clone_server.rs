//! Duplicating a server (`clone_server_row`), all or nothing.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use clone_server::*`.

use crate::*;

/// The copy itself, all or nothing: when any step fails nothing of it stays.
/// Undone in a savepoint rather than by deleting the copy afterwards, as a
/// delete would leave a sync tombstone for a server no other device ever saw.
pub(crate) fn clone_server_row(conn: &rusqlite::Connection, id: i32) -> Result<i64, String> {
    conn.execute("SAVEPOINT clone_server", [])
        .map_err(|e| format!("[DATABASE] SERVER_CLONE_FAILED: {}", e))?;
    let result = (|| {
        let affected = conn.execute(
            "INSERT INTO servers (name, host, port, username, password, credential_id, folder_id, proxy_type, proxy_host, proxy_port, tunnels, auth_type, key_id, autostart, mirrors, color, notes, run_on_connect, run_on_connect_every, jump_host_id, terminal_theme, highlight, protocol)
             SELECT name || ' (copy)', host, port, username, password, credential_id, folder_id, proxy_type, proxy_host, proxy_port, tunnels, auth_type, key_id, 0, mirrors, color, notes, run_on_connect, run_on_connect_every, jump_host_id, terminal_theme, highlight, protocol
             FROM servers WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| format!("[DATABASE] SERVER_CLONE_FAILED: {}", e))?;
        if affected == 0 {
            return Err(format!("[DATABASE] SERVER_CLONE_FAILED: no row with id {}", id));
        }
        let new_id = conn.last_insert_rowid();
        // Place the clone at the bottom of its folder's manual order (the INSERT
        // above doesn't copy `position`, so without this it would default to 0 and
        // jump to the top). Same append logic as add_server, folder taken from the
        // clone's own row.
        let _ = conn.execute(
            "UPDATE servers SET position = COALESCE((SELECT MAX(position) FROM servers s2 WHERE s2.id != servers.id AND s2.folder_id IS servers.folder_id), -1) + 1 WHERE id = ?1",
            rusqlite::params![new_id],
        );
        // What the original wasn't allowed to run yet (a shared profile's held
        // commands and forwards) stays held on the copy, so duplicating isn't a
        // way around the approval. See approvals::copy_held. A duplicate that
        // came without its held entries would run what the original couldn't,
        // so a failure here takes the copy back too.
        approvals::copy_held(conn, id as i64, new_id)?;
        Ok(new_id)
    })();
    match result {
        Ok(new_id) => {
            conn.execute("RELEASE clone_server", [])
                .map_err(|e| format!("[DATABASE] SERVER_CLONE_FAILED: {}", e))?;
            Ok(new_id)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO clone_server; RELEASE clone_server;");
            Err(e)
        }
    }
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
                ("", "mod clone_server;", 1),
                ("", "use clone_server::*;", 1),
                ("clone_server", "clone_server_row(", 1),
            ],
        );
    }
}

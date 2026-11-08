//! The fork's parts of the sync engine's records (lib.rs, collect_local_records
//! and apply_entity), kept here so a sync with the original project doesn't
//! conflict on them.

use crate::{approvals, rdp_links, tree_moves};
use rusqlite::Connection;

/// Nullable synced columns added after their entity already synced. A peer on
/// an older build sends records without the key, which means "this build
/// doesn't know the field", not "clear it" — so the merge keeps this device's
/// value then (and leaves it NULL on a fresh insert). `position` has its own
/// shim in apply_entity, as it is NOT NULL. `scope` is a command's (#185).
/// `protocol` is a server's type (#173: NULL for SSH, "rdp", "telnet",
/// "vnc"): a peer from before it doesn't turn this device's RDP, telnet or
/// VNC server back into an SSH one.
pub(crate) const LATE_COLS: &[&str] = &["terminal_theme", "highlight", "scope", "protocol", "run_on_connect_every"];

/// LATE_COLS whose NULL means "not known here", and so is left out of what
/// this device sends, as an older peer leaves it out. A server's row inserted
/// from an older peer's record has them NULL; sent on as null (the store sync
/// sends every row each time, and a merge may pick that copy), it would
/// clear the scheme or highlight set on a device that has one. The app's
/// default is stored as "" instead (`set_server_look`), which is sent.
pub(crate) const NULL_IS_UNKNOWN: &[&str] = &["terminal_theme", "highlight"];

/// A row of `table` as this device sends it (`payload`, its JSON). A server's
/// is changed: what this device holds back stays as everywhere else, and
/// what it doesn't know it doesn't send. Any other row goes as it is.
pub(crate) fn outgoing(conn: &Connection, table: &str, uuid: &str, payload: String) -> Result<String, String> {
    if table != "servers" {
        return Ok(payload);
    }
    let mut obj: serde_json::Value = serde_json::from_str(&payload).map_err(|e| format!("[SYNC] SER_ROW {}: {e}", table))?;
    // A server whose switch to telnet or VNC this device holds still
    // has that protocol and port everywhere else: an edit here mustn't
    // undo it (approvals::held_switch).
    if let Some(switch) = approvals::held_switch(conn, uuid) {
        obj["protocol"] = serde_json::Value::String(switch.protocol);
        if let Some(port) = switch.port {
            obj["port"] = serde_json::Value::from(port);
        }
    }
    // What this device doesn't know, it doesn't send (NULL_IS_UNKNOWN).
    if let Some(o) = obj.as_object_mut() {
        o.retain(|k, v| !(v.is_null() && NULL_IS_UNKNOWN.contains(&k.as_str())));
    }
    Ok(obj.to_string())
}

/// After a sync merge (`r`, what it returned), with the merge flag down: what
/// the merge may have left that the fork puts right, as this device's edits,
/// which the next push carries back.
pub(crate) fn after_merge(conn: &Connection, r: Result<usize, String>) -> Result<usize, String> {
    let skipped = r?;
    // Folder moves made on two devices can merge into a loop; undone here, as
    // this device's edit, which the next push carries back (tree_moves.rs).
    tree_moves::break_folder_loops(conn).map_err(|e| format!("[SYNC] FOLDER_LOOP: {e}"))?;
    // A Remote Desktop link from a device on an older version becomes an RDP
    // server here, as this device's own did (rdp_links.rs).
    rdp_links::move_into_servers(conn)?;
    Ok(skipped)
}

/// Run `f` with the sync auto-stamp triggers held back (the `merge` flag the
/// triggers' GUARD reads), so what it writes isn't stamped as a local edit to
/// push: a sync merge, or a value only this device keeps (server_os.rs).
pub(crate) fn with_merge_flag<T>(conn: &Connection, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
    conn.execute("INSERT INTO sync_flags(key,val) VALUES('merge',1) ON CONFLICT(key) DO UPDATE SET val=1", [])
        .map_err(|e| format!("[SYNC] MERGE_ON: {e}"))?;
    let r = f(conn);
    let _ = conn.execute("UPDATE sync_flags SET val=0 WHERE key='merge'", []);
    r
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the line that brings the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod sync_merge;", 1),
                ("collect_local_records", "sync_merge::outgoing(conn, spec.table, &uuid, payload)?", 1),
                ("apply_entity", "sync_merge::LATE_COLS.contains(c)", 1),
                ("apply_remote_records", "sync_merge::after_merge(conn, r)", 1),
            ],
        );
    }
}

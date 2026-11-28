//! Secret Library command review guard (#286).
//!
//! When a shared profile syncs a Secret Library command modified by someone else,
//! running it without revealing it first could execute an uninspected change.
//! This module computes whether each command's last change came from someone
//! other than this user so the confirmation dialog keeps **Run** disabled until
//! **Show command** is clicked and displays who changed it and when.

use rusqlite::Connection;
use serde_json::{json, Value};

/// Extract the HLC node id segment from `"{phys}:{counter}:{node}"`.
fn hlc_node_of(stamp: Option<&str>) -> Option<&str> {
    let s = stamp?.trim();
    let mut parts = s.splitn(3, ':');
    let _phys = parts.next()?;
    let _ctr = parts.next()?;
    let node = parts.next()?.trim();
    (!node.is_empty()).then_some(node)
}

/// True when a secret command's last modification came from someone other than
/// the local user:
/// - If `edited_by` is present and `my_label` is known, mismatch means other.
/// - If `my_label` is missing and `edited_by` is present, only treat remote-node
///   writes in a shared profile as other (avoids flagging local signed-out edits).
/// - If `edited_by` is missing/empty in a shared profile, treat a remote-node
///   `updated_at` as a sync change from another member ("someone else").
fn is_changed_by_other(
    is_shared: bool,
    my_label: Option<&str>,
    my_node: Option<&str>,
    is_secret: bool,
    edited_by: Option<&str>,
    updated_at: Option<&str>,
) -> bool {
    if !is_secret {
        return false;
    }
    let from_remote_node = matches!((hlc_node_of(updated_at), my_node), (Some(rn), Some(mn)) if !mn.is_empty() && rn != mn);
    match edited_by.map(str::trim).filter(|s| !s.is_empty()) {
        Some(who) => match my_label.map(str::trim).filter(|s| !s.is_empty()) {
            Some(me) => !who.eq_ignore_ascii_case(me),
            None => is_shared && from_remote_node,
        },
        None => is_shared && from_remote_node,
    }
}

/// Load Library commands from `conn`, enriching each entry with provenance
/// (`edited_by`, `updated_at`) and `changed_by_other` for secret commands.
pub(crate) fn list_commands(conn: &Connection, my_node: Option<&str>) -> Result<Vec<Value>, String> {
    let is_shared = crate::approvals::profile_is_shared(conn);
    let my_label: Option<String> = conn
        .query_row("SELECT value FROM sync_meta WHERE key='editor_label'", [], |r| r.get(0))
        .ok();

    let mut stmt = conn
        .prepare("SELECT id, title, content, scope, is_secret, tags, updated_at, edited_by FROM commands")
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let id = row.get::<_, i32>(0)?;
            let title = row.get::<_, String>(1)?;
            let content = row.get::<_, String>(2)?;
            // As a synced peer sent it: the frontend reads it defensively
            // (util/commandScope), so a broken one counts as all servers.
            let scope = row.get::<_, Option<String>>(3)?;
            // A NULL or out-of-range value from a peer must not fail
            // the whole list (same shape as run_on_connect_every).
            let is_secret = row.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0;
            let tags = row.get::<_, Option<String>>(5)?;
            let updated_at = row.get::<_, Option<String>>(6)?;
            let edited_by = row.get::<_, Option<String>>(7)?;
            let trimmed_editor = edited_by
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let changed_by_other = is_changed_by_other(
                is_shared,
                my_label.as_deref(),
                my_node,
                is_secret,
                trimmed_editor.as_deref(),
                updated_at.as_deref(),
            );
            Ok(json!({
                "id": id,
                "title": title,
                "content": content,
                "scope": scope,
                "is_secret": is_secret,
                "tags": tags,
                "updated_at": updated_at,
                "edited_by": trimmed_editor,
                "changed_by_other": changed_by_other,
            }))
        })
        .map_err(|e| e.to_string())?;

    let mut list = Vec::new();
    for r in rows {
        list.push(r.map_err(|e| format!("[DATABASE] ROW_FETCH_FAILED: {}", e))?);
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork_hooks::app_table;

    fn test_db(shared: bool, editor_label: Option<&str>) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&app_table("commands")).unwrap();
        conn.execute("CREATE TABLE sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)", []).unwrap();
        if shared {
            conn.execute("INSERT INTO sync_meta(key, value) VALUES('share_id', 'share-1')", []).unwrap();
        }
        if let Some(lbl) = editor_label {
            conn.execute("INSERT INTO sync_meta(key, value) VALUES('editor_label', ?1)", [lbl]).unwrap();
        }
        conn
    }

    #[test]
    fn own_secret_across_devices_does_not_require_reveal() {
        let conn = test_db(true, Some("bob@example.com"));
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('t', 'secret-cmd', 1, '0001700000000000:00001:otherdevice', 'Bob@Example.com')",
            [],
        )
        .unwrap();
        let rows = list_commands(&conn, Some("mydevice")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["changed_by_other"], false);
    }

    #[test]
    fn teammate_edited_secret_requires_reveal_even_after_unshare() {
        for shared in [true, false] {
            let conn = test_db(shared, Some("bob@example.com"));
            conn.execute(
                "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('t', 'rm -rf /tmp/x', 1, '0001700000000000:00001:alicedev', 'alice@example.com')",
                [],
            )
            .unwrap();
            let rows = list_commands(&conn, Some("bobdev")).unwrap();
            assert_eq!(rows[0]["changed_by_other"], true, "shared={shared}");
            assert_eq!(rows[0]["edited_by"], "alice@example.com");
        }
    }

    #[test]
    fn unattributed_remote_sync_edit_in_shared_profile_requires_reveal() {
        let conn = test_db(true, None);
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('t', 'curl evil', 1, '0001700000000000:00002:peernode', NULL)",
            [],
        )
        .unwrap();
        let rows = list_commands(&conn, Some("localnode")).unwrap();
        assert_eq!(rows[0]["changed_by_other"], true);
        assert!(rows[0]["edited_by"].is_null());

        // Same-node local edit with no editor_label is treated as own.
        conn.execute("UPDATE commands SET updated_at='0001700000000001:00001:localnode'", []).unwrap();
        let rows_local = list_commands(&conn, Some("localnode")).unwrap();
        assert_eq!(rows_local[0]["changed_by_other"], false);
    }

    #[test]
    fn personal_unattributed_and_non_secret_commands_do_not_trigger() {
        let conn = test_db(false, None);
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('p', 'echo hi', 1, '0001700000000000:00001:other', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('pub', 'uptime', 0, '0001700000000000:00001:other', 'alice@example.com')",
            [],
        )
        .unwrap();
        let rows = list_commands(&conn, Some("local")).unwrap();
        assert_eq!(rows[0]["changed_by_other"], false);
        assert_eq!(rows[1]["changed_by_other"], false);
    }
}

#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod secret_cmd_review;", 1),
                ("get_commands", "secret_cmd_review::list_commands(conn, my_node.as_deref())", 1),
            ],
        );
        assert_hooks(
            "hlc.rs",
            include_str!("hlc.rs"),
            &[("node_id", "&self.node_id", 1)],
        );
    }
}

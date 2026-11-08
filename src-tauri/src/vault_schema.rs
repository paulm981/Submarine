//! The fork's own tables and columns in a profile's vault, added when it's
//! opened (lib.rs, setup_master_db_with), for an existing vault and a fresh
//! one alike, after the original project's own schema and before the
//! sync backfill and triggers. Each step is additive: an older build simply
//! doesn't see what it adds.

use crate::{ai, highlight, multiexec, rdp_links, server_os};
use rusqlite::Connection;

/// Adds them all, in order. True when the vault needs saving again for it
/// (the AI's approval levels were set up just now).
pub(crate) fn add_fork_schema(conn: &Connection, app: &tauri::AppHandle) -> Result<bool, String> {
    add_fork_schema_with(conn, || ai::trust::install_id(app))
}

/// `add_fork_schema`, with this install's id asked for only when the AI's
/// tables are made, as before.
fn add_fork_schema_with(conn: &Connection, install_id: impl FnOnce() -> String) -> Result<bool, String> {
    // Remote Desktop links (rdp_links.rs), for existing and fresh vaults alike: one
    // `CREATE TABLE IF NOT EXISTS` here instead of a copy in each branch
    // above, which is how a fresh vault once went without it. Additive, like
    // run_on_connect: an older build simply doesn't see it.
    conn.execute(rdp_links::SCHEMA, []).map_err(|e| format!("[DATABASE] RDP_TABLE_FAILED: {}", e))?;
    // Its sync columns came later (links sync like servers do); links made
    // before them get their ids from the backfill below.
    for stmt in [
        "ALTER TABLE rdp_links ADD COLUMN uuid TEXT",
        "ALTER TABLE rdp_links ADD COLUMN updated_at TEXT",
        "ALTER TABLE rdp_links ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE rdp_links ADD COLUMN edited_by TEXT",
    ] {
        if let Err(e) = conn.execute(stmt, []) {
            let s = e.to_string();
            if !s.contains("duplicate column name") {
                return Err(format!("[DATABASE] RDP_SYNC_MIGRATION_FAILED: {}", s));
            }
        }
    }
    // The user's own highlight rules (highlight.rs): synced, made with their
    // sync columns, before the backfill and triggers below.
    conn.execute(highlight::SCHEMA, []).map_err(|e| format!("[DATABASE] HIGHLIGHT_TABLE_FAILED: {}", e))?;
    // Saved walls (multiexec.rs): device-local too, added the same way. No
    // longer used, but kept, so a profile's saved walls aren't lost.
    conn.execute(multiexec::SCHEMA, []).map_err(|e| format!("[DATABASE] WALLS_TABLE_FAILED: {}", e))?;
    // The AI's approval levels and rules (ai/trust.rs, #115): device-local
    // too, so a shared profile never changes how much the AI may do here,
    // and tied to this install, so a copy of the vault doesn't carry them.
    let resave = ai::trust::create_schema(conn, &install_id())?;
    // The OS each server last reported (server_os.rs), for its badge.
    // Device-local, not a synced column. Added here for both vault paths too;
    // on a vault that has it already this answers "duplicate column".
    if let Err(e) = conn.execute("ALTER TABLE servers ADD COLUMN os TEXT", []) {
        let s = e.to_string();
        if !s.contains("duplicate column name") {
            return Err(format!("[DATABASE] SERVER_OS_MIGRATION_FAILED: {}", s));
        }
    }
    conn.execute_batch(server_os::RESET_TRIGGER)
        .map_err(|e| format!("[DATABASE] SERVER_OS_TRIGGER_FAILED: {}", e))?;
    // The server's own terminal color scheme (an id from
    // src/ui/terminalThemes.ts); "" = the app's default, NULL = never set
    // (or set on an older peer only). Synced, and in LATE_COLS and
    // NULL_IS_UNKNOWN, so an older peer's edit doesn't clear it.
    if let Err(e) = conn.execute("ALTER TABLE servers ADD COLUMN terminal_theme TEXT", []) {
        let s = e.to_string();
        if !s.contains("duplicate column name") {
            return Err(format!("[DATABASE] SERVER_THEME_MIGRATION_FAILED: {}", s));
        }
    }
    // The server's own highlight set (an id from src/ui/highlight.ts); "" or
    // NULL = this device's. Synced and in LATE_COLS, like terminal_theme.
    if let Err(e) = conn.execute("ALTER TABLE servers ADD COLUMN highlight TEXT", []) {
        let s = e.to_string();
        if !s.contains("duplicate column name") {
            return Err(format!("[DATABASE] SERVER_HIGHLIGHT_MIGRATION_FAILED: {}", s));
        }
    }

    Ok(resave)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a vault made by the app's own statement, twice over (a vault
    /// that has it all already answers "duplicate column", which is fine).
    #[test]
    fn the_fork_schema_goes_on_a_vault_and_again() {
        let conn = Connection::open_in_memory().unwrap();
        for table in ["folders", "servers"] {
            conn.execute_batch(&crate::fork_hooks::app_table(table)).unwrap();
        }
        add_fork_schema_with(&conn, || "this-install-0000".into()).unwrap();
        add_fork_schema_with(&conn, || "this-install-0000".into()).unwrap();
        for col in ["os", "terminal_theme", "highlight"] {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM pragma_table_info('servers') WHERE name = ?1", [col], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "{col}");
        }
        for table in ["rdp_links", "highlight_rules", "walls"] {
            conn.execute(&format!("SELECT * FROM {table}"), []).unwrap();
        }
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_table_info('rdp_links') WHERE name = 'uuid'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
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
                ("", "mod vault_schema;", 1),
                ("setup_master_db_with", "vault_schema::add_fork_schema(&conn, &app_handle)?", 1),
            ],
        );
    }
}

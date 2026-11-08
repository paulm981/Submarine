//! A server's look: its colour scheme, terminal theme and highlighting set.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use server_look::*`.

use crate::*;

/// Keep only what a scheme id can contain (lowercase letters, digits, dashes),
/// at most 64 of them; empty = none.
pub(crate) fn clean_scheme_id(raw: &str) -> Option<String> {
    let id: String = raw.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-').take(64).collect();
    (!id.is_empty()).then_some(id)
}

/// Set a server's own scheme or highlight set (`column`, one of
/// NULL_IS_UNKNOWN). `None`, the app's default, is stored as "": a NULL
/// there means "not known here", which isn't sent to the other devices.
pub(crate) fn set_server_look(conn: &Connection, column: &str, id: i32, value: Option<&str>) -> rusqlite::Result<usize> {
    debug_assert!(crate::sync_merge::NULL_IS_UNKNOWN.contains(&column));
    let value = value.and_then(clean_scheme_id).unwrap_or_default();
    // No write when nothing changed: every save of the edit panel calls this,
    // and an UPDATE would restamp the row for sync even with the same value.
    // Nor is a NULL turned into "": the edit panel shows the default for
    // both, and saving it isn't a choice made here.
    conn.execute(
        &format!("UPDATE servers SET {column}=?1 WHERE id=?2 AND COALESCE({column}, '') IS NOT ?1"),
        rusqlite::params![value, id],
    )
}

/// A server's own terminal color scheme, or `None` for the app's default.
/// Its own command, like set_server_run_on_connect.
#[tauri::command]
pub(crate) async fn set_server_terminal_theme(state: tauri::State<'_, DbState>, id: i32, value: Option<String>) -> Result<(), String> {
    let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
    let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
    set_server_look(conn, "terminal_theme", id, value.as_deref()).map_err(|e| format!("[DATABASE] SERVER_THEME_FAILED: {}", e))?;
    drop(conn_guard);
    save_vault_internal(&state)?;
    Ok(())
}

/// A server's own highlight set, or `None` for this device's. Its own
/// command, like set_server_terminal_theme.
#[tauri::command]
pub(crate) async fn set_server_highlight(state: tauri::State<'_, DbState>, id: i32, value: Option<String>) -> Result<(), String> {
    let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
    let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
    set_server_look(conn, "highlight", id, value.as_deref()).map_err(|e| format!("[DATABASE] SERVER_HIGHLIGHT_FAILED: {}", e))?;
    drop(conn_guard);
    save_vault_internal(&state)?;
    Ok(())
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
                ("", "mod server_look;", 1),
                ("", "use server_look::*;", 1),
                ("run", "set_server_terminal_theme", 1),
                ("run", "set_server_highlight", 1),
            ],
        );
    }
}

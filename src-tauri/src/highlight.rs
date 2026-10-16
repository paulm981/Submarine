//! The user's own highlight rules: text in a terminal to show in a color
//! (src/ui/highlight.ts does the highlighting; this keeps the rules).
//!
//! Rules live in the vault's `highlight_rules` table and sync with the
//! profile like commands and notes do (lib.rs `ENTITIES`). A build that
//! doesn't know the table ignores its records, and keeps them in the store.
//! Which set of rules a terminal uses is the device's choice, or the
//! server's own (`servers.highlight`).

use crate::DbState;
use rusqlite::{params, Connection};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS highlight_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern TEXT NOT NULL,
    kind TEXT NOT NULL DEFAULT 'word',
    match_case INTEGER NOT NULL DEFAULT 0,
    color TEXT NOT NULL DEFAULT 'red',
    bold INTEGER NOT NULL DEFAULT 0,
    underline INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    position INTEGER NOT NULL DEFAULT 0,
    uuid TEXT,
    updated_at TEXT,
    deleted INTEGER NOT NULL DEFAULT 0,
    edited_by TEXT
)";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HighlightRule {
    #[serde(default)]
    pub id: Option<i64>,
    pub pattern: String,
    /// "word" (whole words), "text" (anywhere) or "regex".
    pub kind: String,
    #[serde(default)]
    pub match_case: bool,
    pub color: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// The page checks a rule more closely (a regular expression is tried for
/// speed there); this keeps out what no build could use.
pub(crate) fn validate(rule: &HighlightRule) -> Result<(), String> {
    if rule.pattern.trim().is_empty() {
        return Err("Type the text to highlight.".into());
    }
    if rule.pattern.chars().count() > 500 {
        return Err("That's too long; keep it under 500 characters.".into());
    }
    if rule.pattern.chars().any(|c| c.is_control() && c != '\t') {
        return Err("The text can't contain line breaks.".into());
    }
    if !matches!(rule.kind.as_str(), "word" | "text" | "regex") {
        return Err("Choose how the text matches.".into());
    }
    if rule.color.is_empty() || rule.color.len() > 32 || !rule.color.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
        return Err("Choose a color.".into());
    }
    Ok(())
}

pub(crate) fn list(conn: &Connection) -> rusqlite::Result<Vec<HighlightRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, pattern, kind, match_case, color, bold, underline, enabled FROM highlight_rules ORDER BY position, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(HighlightRule {
            id: Some(r.get(0)?),
            pattern: r.get(1)?,
            kind: r.get(2)?,
            match_case: r.get::<_, i64>(3)? != 0,
            color: r.get(4)?,
            bold: r.get::<_, i64>(5)? != 0,
            underline: r.get::<_, i64>(6)? != 0,
            enabled: r.get::<_, i64>(7)? != 0,
        })
    })?;
    rows.collect()
}

/// Adds the rule at the end of the list, or updates it; its id.
pub(crate) fn save(conn: &Connection, rule: &HighlightRule) -> Result<i64, String> {
    validate(rule)?;
    let db = |e: rusqlite::Error| format!("[DATABASE] HIGHLIGHT_SAVE_FAILED: {}", e);
    match rule.id {
        Some(id) => {
            // Only when something changed: an update restamps the row for sync.
            conn.execute(
                "UPDATE highlight_rules SET pattern=?1, kind=?2, match_case=?3, color=?4, bold=?5, underline=?6, enabled=?7
                 WHERE id=?8 AND (pattern IS NOT ?1 OR kind IS NOT ?2 OR match_case IS NOT ?3 OR color IS NOT ?4
                                  OR bold IS NOT ?5 OR underline IS NOT ?6 OR enabled IS NOT ?7)",
                params![rule.pattern, rule.kind, rule.match_case, rule.color, rule.bold, rule.underline, rule.enabled, id],
            )
            .map_err(db)?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO highlight_rules (pattern, kind, match_case, color, bold, underline, enabled, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, (SELECT COALESCE(MAX(position), -1) + 1 FROM highlight_rules))",
                params![rule.pattern, rule.kind, rule.match_case, rule.color, rule.bold, rule.underline, rule.enabled],
            )
            .map_err(db)?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// The rules in this order (the first wins where two match the same text).
/// Ids not given keep their place after these; rows already in place aren't
/// written, so they aren't sent again.
pub(crate) fn reorder(conn: &Connection, ids: &[i64]) -> Result<usize, String> {
    let mut changed = 0;
    for (pos, id) in ids.iter().enumerate() {
        changed += conn
            .execute(
                "UPDATE highlight_rules SET position=?1 WHERE id=?2 AND position IS NOT ?1",
                params![pos as i64, id],
            )
            .map_err(|e| format!("[DATABASE] HIGHLIGHT_ORDER_FAILED: {}", e))?;
    }
    Ok(changed)
}

fn with_conn<T>(state: &tauri::State<'_, DbState>, f: impl FnOnce(&Connection) -> Result<(T, bool), String>) -> Result<T, String> {
    let (out, changed) = {
        let conn_guard = state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = conn_guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        let tx = conn.unchecked_transaction().map_err(|e| format!("[DATABASE] TRANSACTION_FAILED: {}", e))?;
        let out = f(&tx)?;
        tx.commit().map_err(|e| format!("[DATABASE] TRANSACTION_FAILED: {}", e))?;
        out
    };
    if changed {
        crate::save_vault_internal(state)?;
    }
    Ok(out)
}

#[tauri::command]
pub async fn highlight_rules_list(state: tauri::State<'_, DbState>) -> Result<Vec<HighlightRule>, String> {
    with_conn(&state, |conn| {
        list(conn).map(|l| (l, false)).map_err(|e| format!("[DATABASE] HIGHLIGHT_LIST_FAILED: {}", e))
    })
}

#[tauri::command]
pub async fn highlight_rule_save(state: tauri::State<'_, DbState>, rule: HighlightRule) -> Result<i64, String> {
    with_conn(&state, |conn| save(conn, &rule).map(|id| (id, true)))
}

#[tauri::command]
pub async fn highlight_rule_delete(state: tauri::State<'_, DbState>, id: i64) -> Result<(), String> {
    with_conn(&state, |conn| {
        conn.execute("DELETE FROM highlight_rules WHERE id=?1", params![id])
            .map(|n| ((), n > 0))
            .map_err(|e| format!("[DATABASE] HIGHLIGHT_DELETE_FAILED: {}", e))
    })
}

#[tauri::command]
pub async fn highlight_rules_reorder(state: tauri::State<'_, DbState>, ids: Vec<i64>) -> Result<(), String> {
    with_conn(&state, |conn| reorder(conn, &ids).map(|n| ((), n > 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, kind: &str) -> HighlightRule {
        HighlightRule {
            id: None,
            pattern: pattern.into(),
            kind: kind.into(),
            match_case: false,
            color: "red".into(),
            bold: true,
            underline: false,
            enabled: true,
        }
    }

    #[test]
    fn rules_save_list_and_reorder() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let a = save(&conn, &rule("deploy", "word")).unwrap();
        let b = save(&conn, &rule(r"E\d+", "regex")).unwrap();
        assert_eq!(list(&conn).unwrap().iter().map(|r| r.id.unwrap()).collect::<Vec<_>>(), vec![a, b]);
        let mut edited = rule("deployed", "text");
        edited.id = Some(a);
        save(&conn, &edited).unwrap();
        assert_eq!(list(&conn).unwrap()[0], edited);
        assert_eq!(reorder(&conn, &[b, a]).unwrap(), 2);
        assert_eq!(reorder(&conn, &[b, a]).unwrap(), 0, "rows already in place aren't written");
        assert_eq!(list(&conn).unwrap().iter().map(|r| r.id.unwrap()).collect::<Vec<_>>(), vec![b, a]);
    }

    #[test]
    fn unusable_rules_are_refused() {
        assert!(validate(&rule("  ", "word")).is_err());
        assert!(validate(&rule("a\nb", "word")).is_err());
        assert!(validate(&rule("ok", "glob")).is_err());
        assert!(validate(&rule(&"x".repeat(501), "text")).is_err());
        let mut bad_color = rule("ok", "word");
        bad_color.color = "red; DROP".into();
        assert!(validate(&bad_color).is_err());
        assert!(validate(&rule("a\tb", "text")).is_ok());
    }
}

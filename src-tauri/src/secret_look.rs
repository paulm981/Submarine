//! A secret Library command someone else changed (#286): its Run waits in
//! the box that asks first until the command has been shown there.
//!
//! "You" is this device, and the account it's signed in with: the label
//! (`sync_meta.editor_label`, the cloud email) that every edit made here is
//! stamped with in `edited_by` (lib.rs, `create_sync_triggers`). The
//! vault keeps, for each command whose text a sync brought from anyone else,
//! who that record says changed it and when (`secret_looks`). The table is
//! this device's own: it isn't synced, so nothing a peer sends can take a
//! command off it here.
//!
//! - A sync that brings a new command, or changes a command's text, from a
//!   record whose `edited_by` isn't this device's account puts the command
//!   on it; one from this account (your other computer) takes it off. A
//!   record with no `edited_by` (a device that isn't signed in) is someone
//!   else's: nothing says it's yours.
//! - Saving its text here takes it off.
//! - Running it after Show command takes it off, while the text that ran is
//!   still the text saved (`seen`). It's back on at the next change from
//!   someone else, and only then.
//! - A rename, new tags or the secret switch, from anyone, changes no text
//!   and changes nothing here.
//!
//! The name is what the peer's app wrote, so a peer can lie about it, even
//! claim to be you: this is a nudge to look before a run, not a guard. It's
//! untrusted text at the edge: bounded, without control or invisible
//! characters, and shown by the page as text.

use crate::*;
use rusqlite::{params, Connection, OptionalExtension};

/// The most of a name the box shows, in characters.
pub(crate) const NAME_MAX: usize = 80;

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS secret_looks (
    command_id INTEGER PRIMARY KEY,
    changed_by TEXT,
    stamp TEXT
)";

/// A sync merge is writing (lib.rs, `apply_remote_records`), not this device.
const MERGING: &str = "COALESCE((SELECT val FROM sync_flags WHERE key='merge'),0)<>0";

/// The row's last editor is this device's account. NULL on either side is
/// not a match.
const MINE: &str = "COALESCE(lower(trim(NEW.edited_by)) = lower((SELECT value FROM sync_meta WHERE key='editor_label')), 0)";

fn triggers() -> String {
    let note = format!(
        "INSERT OR REPLACE INTO secret_looks(command_id, changed_by, stamp)
           SELECT NEW.id, NEW.edited_by, NEW.updated_at WHERE {MERGING} AND NOT {MINE};"
    );
    format!(
        "DROP TRIGGER IF EXISTS secret_looks_ai;
         DROP TRIGGER IF EXISTS secret_looks_au;
         DROP TRIGGER IF EXISTS secret_looks_ad;
         CREATE TRIGGER secret_looks_ai AFTER INSERT ON commands FOR EACH ROW
           BEGIN {note} END;
         CREATE TRIGGER secret_looks_au AFTER UPDATE OF content ON commands FOR EACH ROW
           WHEN NEW.content IS NOT OLD.content
           BEGIN
             DELETE FROM secret_looks WHERE command_id = NEW.id AND (NOT {MERGING} OR {MINE});
             {note}
           END;
         CREATE TRIGGER secret_looks_ad AFTER DELETE ON commands FOR EACH ROW
           BEGIN DELETE FROM secret_looks WHERE command_id = OLD.id; END;"
    )
}

/// Makes the table and its triggers, once the sync's own tables are there
/// (lib.rs, `setup_master_db_with`). The first time, the secret commands
/// someone else last changed go on it: those whose `edited_by` names an
/// account that isn't this device's. One with none was made before anyone
/// signed in, most likely here, and stays off. True when it added any, so the
/// vault is saved.
pub(crate) fn create(conn: &Connection) -> Result<bool, String> {
    let db = |e: rusqlite::Error| format!("[DATABASE] SECRET_LOOKS_FAILED: {e}");
    let existed: bool = conn
        .query_row("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='secret_looks')", [], |r| r.get(0))
        .map_err(db)?;
    conn.execute(SCHEMA, []).map_err(db)?;
    conn.execute_batch(&triggers()).map_err(db)?;
    if existed {
        return Ok(false);
    }
    let added = conn
        .execute(
            "INSERT OR IGNORE INTO secret_looks(command_id, changed_by, stamp)
               SELECT id, edited_by, updated_at FROM commands
                WHERE COALESCE(is_secret, 0) <> 0 AND trim(COALESCE(edited_by, '')) <> ''
                  AND NOT COALESCE(lower(trim(edited_by)) = lower((SELECT value FROM sync_meta WHERE key='editor_label')), 0)",
            [],
        )
        .map_err(db)?;
    Ok(added > 0)
}

/// Why a run waits for Show command.
#[derive(serde::Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Why {
    /// Someone else changed it (`changed_by`, `changed_at`).
    Changed,
    /// The list's copy isn't what's saved now (changed or deleted since the
    /// list was loaded): who wrote the copy that would run isn't known.
    Stale,
}

#[derive(serde::Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecretLook {
    pub why: Why,
    /// Who the record says changed it (`shown_name`); None: no name.
    pub changed_by: Option<String>,
    /// When, in milliseconds since 1970, from the record's stamp.
    pub changed_at: Option<u64>,
}

/// A name from a peer as the box may show it: no control or invisible
/// characters (a direction override could make it read as someone else's),
/// trimmed, at most NAME_MAX characters. None when nothing is left.
pub(crate) fn shown_name(raw: Option<&str>) -> Option<String> {
    let invisible = |c: char| {
        c.is_control()
            || matches!(c, '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
    };
    let clean: String = raw?.chars().filter(|c| !invisible(*c)).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return None;
    }
    if clean.chars().count() <= NAME_MAX {
        return Some(clean.to_string());
    }
    let mut cut: String = clean.chars().take(NAME_MAX - 1).collect();
    cut.push('…');
    Some(cut)
}

/// A command's saved text, whether it's on the list, and who and when.
type LookRow = (Option<String>, bool, Option<String>, Option<String>);

/// Whether a run of command `id`, whose text the list holds as `content`,
/// waits for Show command, and why. None: it runs as before.
pub(crate) fn look_for(conn: &Connection, id: i64, content: &str) -> Result<Option<SecretLook>, String> {
    let db = |e: rusqlite::Error| format!("[DATABASE] SECRET_LOOKS_FAILED: {e}");
    let row: Option<LookRow> = conn
        .query_row(
            "SELECT c.content, l.command_id IS NOT NULL, l.changed_by, l.stamp
               FROM commands c LEFT JOIN secret_looks l ON l.command_id = c.id WHERE c.id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(db)?;
    let Some((saved, noted, by, stamp)) = row else {
        return Ok(Some(SecretLook { why: Why::Stale, changed_by: None, changed_at: None }));
    };
    if noted {
        let changed_at = stamp.as_deref().filter(|s| crate::hlc::Hlc::valid_remote(s)).map(crate::hlc::Hlc::phys_of);
        return Ok(Some(SecretLook { why: Why::Changed, changed_by: shown_name(by.as_deref()), changed_at }));
    }
    if saved.as_deref() != Some(content) {
        return Ok(Some(SecretLook { why: Why::Stale, changed_by: None, changed_at: None }));
    }
    Ok(None)
}

/// Command `id` ran after Show command, with `content`: off the list, unless
/// its text changed meanwhile. True when it was on it.
pub(crate) fn seen(conn: &Connection, id: i64, content: &str) -> Result<bool, String> {
    conn.execute(
        "DELETE FROM secret_looks WHERE command_id = ?1
           AND EXISTS (SELECT 1 FROM commands WHERE id = ?1 AND content = ?2)",
        params![id, content],
    )
    .map(|n| n > 0)
    .map_err(|e| format!("[DATABASE] SECRET_LOOKS_FAILED: {e}"))
}

/// The page's question before a secret command's run (src/fork/library).
#[tauri::command]
pub async fn secret_look_get(state: tauri::State<'_, DbState>, id: i64, content: String) -> Result<Option<SecretLook>, String> {
    let conn = std::sync::Arc::clone(&state.conn);
    tokio::task::spawn_blocking(move || {
        let guard = conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        look_for(conn, id, &content)
    })
    .await
    .map_err(|e| format!("[STATE] JOIN_FAILED: {e}"))?
}

/// A run after Show command (`seen`), saved when it changed anything.
#[tauri::command]
pub async fn secret_look_seen(state: tauri::State<'_, DbState>, id: i64, content: String) -> Result<(), String> {
    let conn = std::sync::Arc::clone(&state.conn);
    let changed = tokio::task::spawn_blocking(move || {
        let guard = conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        seen(conn, id, &content)
    })
    .await
    .map_err(|e| format!("[STATE] JOIN_FAILED: {e}"))??;
    if changed {
        save_vault_async(&state).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_engine_tests::device;

    const KEY: [u8; 32] = [7u8; 32];

    fn sync(from: &Connection, to: &Connection, hlc: &crate::hlc::Hlc) {
        crate::apply_remote_records(to, &KEY, &crate::collect_local_records(from, &KEY, "").unwrap(), hlc).unwrap();
    }

    fn signed_in(c: &Connection, email: &str) {
        c.execute("INSERT OR REPLACE INTO sync_meta(key,value) VALUES('editor_label',?1)", [email]).unwrap();
    }

    fn id_of(c: &Connection, title: &str) -> i64 {
        c.query_row("SELECT id FROM commands WHERE title=?1", [title], |r| r.get(0)).unwrap()
    }

    fn content_of(c: &Connection, title: &str) -> String {
        c.query_row("SELECT content FROM commands WHERE title=?1", [title], |r| r.get(0)).unwrap()
    }

    fn look(c: &Connection, title: &str) -> Option<SecretLook> {
        look_for(c, id_of(c, title), &content_of(c, title)).unwrap()
    }

    fn by(name: &str) -> Option<String> {
        Some(name.to_string())
    }

    // Alice's device and Bob's, each signed in, each with the look table.
    fn two() -> ((Connection, std::sync::Arc<crate::hlc::Hlc>), (Connection, std::sync::Arc<crate::hlc::Hlc>)) {
        let (a, ha) = device("A");
        let (b, hb) = device("B");
        signed_in(&a, "alice@x.com");
        signed_in(&b, "bob@x.com");
        create(&a).unwrap();
        create(&b).unwrap();
        ((a, ha), (b, hb))
    }

    fn later() {
        // Stamps have to move on for the next edit to win.
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    // What you save is yours: nothing to look at, here or after it comes back.
    #[test]
    fn your_own_save_runs_as_before() {
        let ((a, ha), (b, hb)) = two();
        a.execute("INSERT INTO commands(title, content, is_secret) VALUES('db', 'mysql -p''x''', 1)", []).unwrap();
        assert_eq!(look(&a, "db"), None);
        sync(&a, &b, &hb);
        sync(&b, &a, &ha);
        assert_eq!(look(&a, "db"), None, "its own record back from the server changes nothing");
        a.execute("UPDATE commands SET content='mysql -p''y''' WHERE title='db'", []).unwrap();
        assert_eq!(look(&a, "db"), None);
    }

    // Bob's new secret, and Bob's change to Alice's, wait for a look on
    // Alice's device, naming Bob and when; his rename doesn't.
    #[test]
    fn someone_else_s_change_waits_for_a_look_and_names_them() {
        let ((a, ha), (b, hb)) = two();
        b.execute("INSERT INTO commands(title, content, is_secret) VALUES('bobs', 'curl -H token:1', 1)", []).unwrap();
        a.execute("INSERT INTO commands(title, content, is_secret) VALUES('mine', 'echo one', 1)", []).unwrap();
        sync(&b, &a, &ha);
        sync(&a, &b, &hb);
        let l = look(&a, "bobs").unwrap();
        assert_eq!((l.why, l.changed_by), (Why::Changed, by("bob@x.com")));
        let stamp: String = a.query_row("SELECT updated_at FROM commands WHERE title='bobs'", [], |r| r.get(0)).unwrap();
        assert_eq!(l.changed_at, Some(crate::hlc::Hlc::phys_of(&stamp)));
        assert_eq!(look(&a, "mine"), None);
        // On Bob's device, Alice's is the one to look at.
        assert_eq!(look(&b, "mine").unwrap().changed_by, by("alice@x.com"));
        assert_eq!(look(&b, "bobs"), None);

        // Bob renames Alice's: no text changed, nothing to look at.
        later();
        b.execute("UPDATE commands SET title='mine2', tags='prod' WHERE title='mine'", []).unwrap();
        sync(&b, &a, &ha);
        assert_eq!(look(&a, "mine2"), None);
        // Bob changes its text: it waits.
        later();
        b.execute("UPDATE commands SET content='echo two' WHERE title='mine2'", []).unwrap();
        sync(&b, &a, &ha);
        assert_eq!(look(&a, "mine2").unwrap().changed_by, by("bob@x.com"));
    }

    // Run after Show: off, until the next change from someone else.
    #[test]
    fn a_run_after_show_clears_it_until_it_changes_again() {
        let ((a, ha), (b, _hb)) = two();
        b.execute("INSERT INTO commands(title, content, is_secret) VALUES('s', 'v1', 1)", []).unwrap();
        sync(&b, &a, &ha);
        let id = id_of(&a, "s");
        // The list's copy is older than what's saved: not cleared.
        assert!(!seen(&a, id, "v0").unwrap());
        assert!(look(&a, "s").is_some());
        assert!(seen(&a, id, "v1").unwrap());
        assert_eq!(look(&a, "s"), None);
        assert_eq!(look(&a, "s"), None, "the next run doesn't ask for a look");
        // His rename after it still doesn't.
        later();
        b.execute("UPDATE commands SET title='s2' WHERE title='s'", []).unwrap();
        sync(&b, &a, &ha);
        assert_eq!(look(&a, "s2"), None);
        later();
        b.execute("UPDATE commands SET content='v2' WHERE title='s2'", []).unwrap();
        sync(&b, &a, &ha);
        assert_eq!(look(&a, "s2").unwrap().why, Why::Changed);
        // Saving it here is a look too.
        a.execute("UPDATE commands SET content='v3' WHERE title='s2'", []).unwrap();
        assert_eq!(look(&a, "s2"), None);
    }

    // Your other computer, signed in as you, is you; one that isn't signed
    // in is "another device", since nothing says it's you.
    #[test]
    fn your_other_device_is_you_and_an_unsigned_one_is_not() {
        let (a, ha) = device("A");
        let (a2, _h2) = device("C");
        signed_in(&a, "Alice@X.com");
        signed_in(&a2, "alice@x.com");
        create(&a).unwrap();
        a2.execute("INSERT INTO commands(title, content, is_secret) VALUES('s', 'v1', 1)", []).unwrap();
        sync(&a2, &a, &ha);
        assert_eq!(look(&a, "s"), None);

        let (u, _hu) = device("U");
        u.execute("INSERT INTO commands(title, content, is_secret) VALUES('u', 'v1', 1)", []).unwrap();
        sync(&u, &a, &ha);
        let l = look(&a, "u").unwrap();
        assert_eq!((l.why, l.changed_by), (Why::Changed, None));
        assert!(l.changed_at.is_some());
        // And signed out here, nothing from a sync is yours.
        let (o, ho) = device("O");
        create(&o).unwrap();
        sync(&a2, &o, &ho);
        assert!(look(&o, "s").is_some());
    }

    // A deletion takes it off; a command the list holds that's gone, or whose
    // text changed since, waits for a look with no name.
    #[test]
    fn a_stale_copy_waits_and_a_deletion_clears() {
        let ((a, ha), (b, _hb)) = two();
        a.execute("INSERT INTO commands(title, content, is_secret) VALUES('s', 'v1', 1)", []).unwrap();
        let id = id_of(&a, "s");
        assert_eq!(look_for(&a, id, "v0").unwrap().map(|l| l.why), Some(Why::Stale));
        b.execute("INSERT INTO commands(title, content, is_secret) VALUES('t', 'v1', 1)", []).unwrap();
        sync(&b, &a, &ha);
        let t = id_of(&a, "t");
        a.execute("DELETE FROM commands WHERE id=?1", [t]).unwrap();
        let left: i64 = a.query_row("SELECT COUNT(*) FROM secret_looks", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        assert_eq!(look_for(&a, t, "v1").unwrap().map(|l| l.why), Some(Why::Stale));
    }

    // The first time, the secrets someone else last changed go on it; a
    // second open adds nothing again.
    #[test]
    fn the_first_open_notes_what_others_changed_once() {
        let (a, _ha) = device("A");
        signed_in(&a, "alice@x.com");
        a.execute_batch(
            "INSERT INTO commands(title, content, is_secret) VALUES('mine', 'a', 1), ('plain', 'b', 0), ('old', 'c', 1), ('bobs', 'd', 1), ('bobsplain', 'e', 0);
             UPDATE commands SET edited_by = NULL WHERE title = 'old';
             UPDATE commands SET edited_by = 'bob@x.com' WHERE title IN ('bobs', 'bobsplain');",
        )
        .unwrap();
        // The UPDATEs above restamped edited_by; put Bob back as the sync would.
        a.execute("INSERT INTO sync_flags(key,val) VALUES('merge',1) ON CONFLICT(key) DO UPDATE SET val=1", []).unwrap();
        a.execute_batch(
            "UPDATE commands SET edited_by = NULL WHERE title = 'old';
             UPDATE commands SET edited_by = 'bob@x.com' WHERE title IN ('bobs', 'bobsplain');",
        )
        .unwrap();
        a.execute("UPDATE sync_flags SET val=0 WHERE key='merge'", []).unwrap();
        assert!(create(&a).unwrap());
        let noted: Vec<String> = {
            let mut s = a.prepare("SELECT c.title FROM secret_looks l JOIN commands c ON c.id = l.command_id ORDER BY c.title").unwrap();
            s.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(noted, vec!["bobs".to_string()]);
        assert!(seen(&a, id_of(&a, "bobs"), "d").unwrap());
        assert!(!create(&a).unwrap(), "only the first time");
        assert_eq!(look(&a, "bobs"), None);
    }

    // On the very table a new vault is made with (its columns by their names),
    // beside the sync's flags and labels, as the app opens a vault.
    #[test]
    fn it_works_on_the_app_s_own_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&crate::fork_hooks::app_table("commands")).unwrap();
        conn.execute_batch(
            "CREATE TABLE sync_flags (key TEXT PRIMARY KEY, val INTEGER NOT NULL);
             CREATE TABLE sync_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .unwrap();
        signed_in(&conn, "alice@x.com");
        assert!(!create(&conn).unwrap());
        conn.execute("INSERT INTO commands(title, content, is_secret) VALUES('mine', 'v1', 1)", []).unwrap();
        assert_eq!(look(&conn, "mine"), None);
        conn.execute("INSERT INTO sync_flags(key,val) VALUES('merge',1) ON CONFLICT(key) DO UPDATE SET val=1", []).unwrap();
        conn.execute(
            "INSERT INTO commands(title, content, is_secret, uuid, updated_at, edited_by) VALUES('bobs', 'v1', 1, 'u1', '000000000000001:00000:B', 'bob@x.com')",
            [],
        )
        .unwrap();
        conn.execute("UPDATE sync_flags SET val=0 WHERE key='merge'", []).unwrap();
        assert_eq!(look(&conn, "bobs"), Some(SecretLook { why: Why::Changed, changed_by: by("bob@x.com"), changed_at: Some(1) }));
    }

    // A hostile name: no control or invisible characters, bounded.
    #[test]
    fn a_name_is_bounded_plain_text() {
        assert_eq!(shown_name(None), None);
        assert_eq!(shown_name(Some("  \u{202E}\u{200B} ")), None);
        assert_eq!(shown_name(Some(" bob@x.com\n")), by("bob@x.com"));
        assert_eq!(shown_name(Some("ev\u{202E}moc.x@bob")), by("evmoc.x@bob"));
        let long = "<b>".repeat(1000);
        let shown = shown_name(Some(&long)).unwrap();
        assert_eq!(shown.chars().count(), NAME_MAX);
        assert!(shown.ends_with('…'));
        assert!(shown.starts_with("<b><b>"), "kept as text: the page shows it as text");
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
                ("", "mod secret_look;", 1),
                ("setup_master_db_with", "secret_look::create(&conn)?", 1),
                ("run", "secret_look::secret_look_get", 1),
                ("run", "secret_look::secret_look_seen", 1),
            ],
        );
    }
}

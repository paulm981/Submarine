//! The fork's parts of the sync engine (lib.rs, collect_local_records,
//! apply_remote_records, apply_remote_inner, resolve_held and apply_entity),
//! kept here so a sync with the original project doesn't conflict on them:
//! one batch's merge (`merge_batch`), what comes after it, and the late
//! columns.

use crate::*;
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

/// One sync batch, inside the merge's transaction `tx`: the records, then the
/// deletions, then everything that's checked once they're all in (links, the
/// holds of a shared profile). Returns `skipped` plus the records that didn't
/// apply. With `hold` false the records are ones held before (a wipe, a big
/// deletion) that this device's user said yes to: those aren't held again,
/// but whatever else in them runs by itself, or lets the AI do more, still is.
pub(crate) fn merge_batch(tx: &Connection, key: &[u8; 32], records: &[&SyncRecord], mut skipped: usize, hold: bool) -> Result<usize, String> {
    // FKs to re-resolve once the whole batch has landed. `ref_table` has to ride
    // along: the lookup used to read `SELECT id FROM {table}` — the row's OWN
    // table — which was only ever correct because the sole queued FKs were the
    // self-referential ones (folders.parent_id, servers.jump_host_id), where the
    // two tables happen to be the same. Any cross-table FK looked up the uuid in
    // entirely the wrong table, found nothing, and NULLed a perfectly good link.
    let mut fk_fixups: Vec<FkFixup> = Vec::new();
    // Per-record application is best-effort: a single record that won't decrypt
    // or parse — a corrupt or hostile blob from a broken/compromised server — is
    // logged and skipped, never aborting the merge. One poison record must not be
    // able to block every OTHER record (or every future sync) from applying;
    // each one counts in `skipped`.
    //
    // In a shared profile, whatever a record newly sets running by itself
    // (commands on connect, autostart, forwards, monitor commands) is held
    // until this device's user allows it — see approvals.rs. Holding is part
    // of the merge's transaction: if it can't be recorded, nothing lands. It's
    // done once the batch is in: a server's jump host lands in the repair pass.
    let approvals_ready = approvals::ready(tx);
    let shared = approvals_ready && approvals::profile_is_shared(tx);
    let mut watched = Vec::new();
    // A viewer's rows go nowhere, so a password they saved here is theirs alone
    // (each person typing their own is a normal way to share a server list),
    // and a record with none says nothing about it. The record lands, without
    // emptying anything (`apply_entity`): held whole, the owner's new address
    // would stop at this device.
    let viewer = shared && approvals::is_viewer(tx);
    // And a move (a server's folder, a folder's parent) that lets the AI do
    // more on a server here is held too (ai/trust.rs): each server's AI
    // policy as it is before the merge, to compare with after it.
    let ai_before = if shared { ai_tabs::trust::before_sync(tx) } else { None };
    // Each server's jump host before the merge: one the merge deletes leaves
    // the servers behind it refusing to go direct (jump_gone.rs).
    let jumps_before = jump_gone::links_before(tx);
    for spec in ENTITIES {
        let watch = shared && approvals::watches(spec.table);
        for rec in records.iter().filter(|r| !r.deleted && r.entity_type == spec.table) {
            // In a shared profile, a record that empties a saved key or
            // password waits for this device's user, like an SSH key's
            // deletion (approvals::hold_wipe). A personal profile takes it.
            if hold && shared && !viewer && empties_a_secret(tx, key, rec) {
                approvals::hold_wipe(tx, rec)?;
                continue;
            }
            // A server's held switch to telnet or VNC stays held after the
            // share is left, like everything else held: outside a share, only
            // that part runs, for a server that has one.
            let held_switch = !watch
                && approvals_ready
                && spec.table == "servers"
                && approvals::held_switch(tx, &rec.uuid).is_some();
            // A held switch's protocol and port go back in the row for the
            // record to merge over, and hold_switch keeps this device's again
            // after it, whether the record applied or not.
            let before = if watch || held_switch { approvals::before_merge(tx, spec.table, &rec.uuid)? } else { None };
            let tunnels_before = if spec.table == "servers" { stored_tunnels(tx, &rec.uuid) } else { None };
            let applied = apply_entity(tx, key, spec, rec, viewer, &mut fk_fixups);
            if let Some(old) = tunnels_before {
                note_synced_tunnels(tx, &rec.uuid, &old)?;
            }
            // Held once the batch is in (below), applied or not: a held
            // switch's row has to get this device's values back either way.
            if watch {
                watched.push((spec.table, &rec.uuid, before));
            } else if held_switch {
                approvals::hold_switch(tx, &rec.uuid, before.as_ref())?;
            }
            if let Err(e) = applied {
                skipped += 1;
                eprintln!("[SYNC] skipped record uuid={} type={}: {}", rec.uuid, rec.entity_type, e);
                continue;
            }
            if shared {
                approvals::release_wipe(tx, rec)?;
            }
        }
    }
    // In a shared profile, a sync that would delete much of it at once, or an
    // SSH key, waits for this device's user (approvals::hold_deletions).
    let tombstones: Vec<&SyncRecord> = records.iter().filter(|r| r.deleted).copied().collect();
    let held = if hold && shared { approvals::hold_deletions(tx, &tombstones)? } else { Default::default() };
    for rec in tombstones.iter().filter(|r| !held.contains(&r.uuid)) {
        if let Err(e) = in_savepoint(tx, || apply_tombstone(tx, rec))? {
            skipped += 1;
            eprintln!("[SYNC] skipped tombstone uuid={} type={}: {}", rec.uuid, rec.entity_type, e);
        }
    }
    // Re-resolve every optional FK against the FINAL state of the batch, now that
    // both the entities and the tombstones have landed.
    repair_links(tx, fk_fixups);
    for (table, uuid, before) in &watched {
        approvals::hold_changes(tx, table, uuid, before.as_ref())?;
    }
    jump_gone::note_lost(tx, &jumps_before).map_err(|e| format!("[SYNC] JUMP_GONE: {e}"))?;
    // Everything in place: hold each server whose AI policy the merge
    // loosened, until this device's user allows it.
    if let Some(before) = ai_before {
        ai_tabs::trust::hold_loosened(tx, before)?;
    }
    Ok(skipped)
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
                ("apply_remote_inner", "sync_merge::merge_batch(&tx, key, &records, refused.len(), true)?", 1),
                ("resolve_held", "sync_merge::merge_batch(&tx, key, &records, 0, false)?", 1),
            ],
        );
    }
}

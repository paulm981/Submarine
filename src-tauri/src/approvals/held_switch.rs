//! What the fork holds, besides what approvals.rs describes.
//!
//! **A server switched to telnet or VNC** from anything else (SSH, RDP) is held
//! for what it would send rather than run. Telnet sends the password as plain
//! text, and VNC answers its password check with weak protection and doesn't
//! check the server's identity, so the switch could take the server's saved
//! password somewhere it could be read. Unlike the others, this device keeps
//! the old protocol and port in the row until it's allowed (`hold_switch` puts
//! them back, and `before_merge` brings the synced ones back for the next
//! record to merge over); what it pushes still carries the synced ones
//! (`held_switch`), so the hold never undoes the change for everyone else. The
//! user can also keep it as it was (`keep_ssh`), which is their own edit and
//! goes to everyone. A switch to RDP isn't held: its password is only sent
//! after the server's certificate is checked, as an SSH password is after its
//! host key. But a switch from RDP to telnet or VNC is, or SSH to RDP to VNC
//! would get round the hold in two steps. Its entry is
//! `protocol:<telnet|vnc>|<port>`.
//!
//! **Entries made while a profile was shared** stay after it's left, like the
//! rest of the device's state, until the user allows them; a held switch goes
//! on being kept through the merges that follow.
//!
//! **A forward is held when a synced record makes it run:** a new one, or one
//! this device's user stopped and someone else started again. One that comes
//! in stopped, or is stopped again, runs nothing and isn't held.
//!
//! The fork's own, moved here out of approvals.rs (#136) so a sync with the
//! original project doesn't conflict on it. approvals.rs brings it back in with
//! `use held_switch::*`.

use super::*;

/// A held switch of protocol: `protocol:telnet|23` or `protocol:vnc|5900`.
pub const PROTOCOL_PREFIX: &str = "protocol:";

/// A held switch: the protocol and port the synced record has for a server,
/// which this device hasn't taken yet. No port: leave the row's alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldSwitch {
    pub protocol: String,
    pub port: Option<i64>,
}

impl HeldSwitch {
    pub(crate) fn item(&self) -> String {
        format!("{PROTOCOL_PREFIX}{}|{}", self.protocol, self.port.map(|p| p.to_string()).unwrap_or_default())
    }

    pub(crate) fn parse(item: &str) -> Option<Self> {
        let rest = item.strip_prefix(PROTOCOL_PREFIX)?;
        let (protocol, port) = rest.split_once('|').unwrap_or((rest, ""));
        Some(Self { protocol: protocol.to_string(), port: port.parse().ok() })
    }
}

/// NULL (or "ssh") is an SSH server.
pub(crate) fn is_ssh(protocol: Option<&str>) -> bool {
    matches!(protocol, None | Some("") | Some("ssh"))
}

/// The protocols a switch to is held for: they'd send the server's saved
/// password where it could be read.
pub(crate) fn sends_password_weakly(protocol: Option<&str>) -> bool {
    matches!(protocol, Some("telnet") | Some("vnc"))
}

/// A server's held switch to telnet or VNC, if there is one.
pub fn held_switch(conn: &Connection, uuid: &str) -> Option<HeldSwitch> {
    held_for(conn, uuid).iter().find_map(|item| HeldSwitch::parse(item))
}

/// The protocol a server's held switch is to.
#[cfg(test)]
pub fn held_protocol(conn: &Connection, uuid: &str) -> Option<String> {
    held_switch(conn, uuid).map(|s| s.protocol)
}

/// Puts the synced protocol and port of a held switch in the row.
pub(crate) fn apply_switch(conn: &Connection, uuid: &str, switch: &HeldSwitch) -> Result<(), String> {
    conn.execute(
        "UPDATE servers SET protocol = ?1, port = COALESCE(?2, port) WHERE uuid = ?3",
        params![switch.protocol, switch.port, uuid],
    )
    .map(|_| ())
    .map_err(|e| format!("[SYNC] HELD_PROTOCOL: {e}"))
}

/// Just before the merge engine applies a record: the row as this device has
/// it (for `hold_changes` / `hold_switch`), and a held switch's protocol and
/// port put back in the row, so the record merges over the synced values
/// rather than the ones this device kept (a peer whose record has no
/// `protocol` key then leaves the synced one). `hold_switch` keeps this
/// device's again afterwards if the switch is still there. Run with the merge
/// flag on, like the merge.
pub fn before_merge(conn: &Connection, table: &str, uuid: &str) -> Result<Option<Snapshot>, String> {
    let before = snapshot(conn, table, uuid);
    if table == "servers" {
        if let Some(switch) = held_switch(conn, uuid) {
            apply_switch(conn, uuid, &switch)?;
        }
    }
    Ok(before)
}

/// After the merge engine applied a server record: an existing server that
/// wasn't telnet or VNC (SSH, RDP) and that the record makes a telnet or VNC
/// one stays what it was here, on its old port, and the switch is held;
/// anything else lets a held switch go. A new server (`before` None) isn't
/// held: the user sees what it is before opening it.
/// Part of `hold_changes`; outside a share, run on its own for a server that
/// has a switch held from before.
pub fn hold_switch(conn: &Connection, uuid: &str, before: Option<&Snapshot>) -> Result<(), String> {
    let Some(Snapshot::Server { protocol: b_protocol, port: b_port, .. }) = before else { return Ok(()) };
    let Some(Snapshot::Server { protocol, port, .. }) = snapshot(conn, "servers", uuid) else { return Ok(()) };
    match protocol {
        Some(protocol) if !sends_password_weakly(b_protocol.as_deref()) && sends_password_weakly(Some(&protocol)) => {
            let item = HeldSwitch { protocol, port }.item();
            release_missing(conn, uuid, PROTOCOL_PREFIX, &BTreeSet::from([item.clone()]))?;
            hold(conn, uuid, &item)?;
            // SSH as plain NULL, whatever spelling it had: an "ssh" here would
            // look like a change to the form, which saves SSH as NULL.
            let kept = b_protocol.as_deref().filter(|p| !is_ssh(Some(p)));
            conn.execute("UPDATE servers SET protocol = ?1, port = ?2 WHERE uuid = ?3", params![kept, b_port, uuid])
                .map(|_| ())
                .map_err(|e| format!("[SYNC] HOLD_PROTOCOL: {e}"))
        }
        _ => release_missing(conn, uuid, PROTOCOL_PREFIX, &BTreeSet::new()),
    }
}

/// The user's "Keep it SSH" (or RDP) for a held switch: the switch is dropped
/// and the server, as it was on its old port here, is saved as this device's
/// own edit, so the next sync takes it back to that everywhere. False when
/// nothing was held.
pub fn keep_ssh(conn: &Connection, uuid: &str) -> Result<bool, String> {
    if held_switch(conn, uuid).is_none() {
        return Ok(false);
    }
    conn.execute("DELETE FROM held_approvals WHERE uuid = ?1 AND item LIKE ?2 || '%'", params![uuid, PROTOCOL_PREFIX])
        .map_err(|e| format!("[SYNC] KEEP_SSH: {e}"))?;
    // Unchanged values, but an update all the same: the sync triggers stamp it.
    conn.execute(
        "UPDATE servers SET protocol = CASE WHEN protocol IN ('', 'ssh') THEN NULL ELSE protocol END, port = port WHERE uuid = ?1",
        [uuid],
    )
        .map_err(|e| format!("[SYNC] KEEP_SSH: {e}"))?;
    Ok(true)
}

/// Refused when on-connect commands are allowed against a text that isn't
/// theirs any more. The frontend looks for `HELD_CHANGED`.
pub const HELD_CHANGED: &str =
    "[SYNC] HELD_CHANGED: These on-connect commands were changed again after they were shown to you. Look at them again before you allow them.";

/// The on-connect commands are held by their item alone, which a newer synced
/// text keeps. So an OK counts only for what the user was shown: `seen` must
/// be the text the row has now, and is needed whenever `allowing` them, and
/// `seen_every` whether they were shown as going into each new terminal (no
/// answer counts as "no"). Anything else is refused (`HELD_CHANGED`), and the
/// user is asked again.
pub fn check_seen_run_on_connect(
    conn: &Connection,
    uuid: &str,
    allowing: bool,
    seen: Option<&str>,
    seen_every: Option<bool>,
) -> Result<(), String> {
    let Some(seen) = seen else {
        return if allowing { Err(HELD_CHANGED.into()) } else { Ok(()) };
    };
    let (now, every): (String, bool) = conn
        .query_row("SELECT COALESCE(run_on_connect, ''), run_on_connect_every FROM servers WHERE uuid = ?1", [uuid], |r| {
            Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0) != 0))
        })
        .map_err(|e| format!("[SYNC] ALLOW: {e}"))?;
    if now == seen && every == seen_every.unwrap_or(false) {
        Ok(())
    } else {
        Err(HELD_CHANGED.into())
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
/// "Keep it SSH" (or RDP) for a server whose switch to telnet or VNC is held:
/// it stays as it was on its old port, saved as this device's own edit, so the
/// next sync takes it back to that everywhere (approvals::keep_ssh). False
/// when no switch was held.
#[tauri::command]
pub(crate) async fn keep_server_ssh(db_state: tauri::State<'_, crate::DbState>, server_id: i32) -> Result<bool, String> {
    let kept = {
        let guard = db_state.conn.lock().map_err(|_| "[STATE] LOCK_FAILED")?;
        let conn = guard.as_ref().ok_or("[STATE] DATABASE_NOT_INITIALIZED")?;
        let uuid = server_uuid(conn, server_id as i64).ok_or("[SYNC] NO_SUCH_SERVER")?;
        keep_ssh(conn, &uuid)?
    };
    if kept {
        crate::save_vault_internal(&db_state)?;
    }
    Ok(kept)
}

// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "approvals.rs",
            include_str!("../approvals.rs"),
            &[
                ("", "mod held_switch;", 1),
                ("", "pub(crate) use held_switch::*;", 1),
                // The fork's tests, in approvals/tests/fork.rs (#215).
                ("", "    mod fork;", 1),
            ],
        );
        assert_hooks("lib.rs", include_str!("../lib.rs"), &[("run", "approvals::keep_server_ssh", 1)]);
        // The sync merge's batch, out of lib.rs into a fork file (#163).
        assert_hooks(
            "sync_merge.rs",
            include_str!("../sync_merge.rs"),
            &[
                ("merge_batch", "approvals::held_switch(", 1),
                ("merge_batch", "approvals::before_merge(", 1),
                ("merge_batch", "approvals::hold_switch(", 1),
                ("outgoing", "approvals::held_switch(", 1),
            ],
        );
    }
}

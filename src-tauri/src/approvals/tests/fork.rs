//! The fork's tests of approvals.rs: held commands, duplicates, forwards,
//! and a synced switch of protocol. Moved here out of the original project's
//! `mod tests` (#215), so a sync doesn't conflict on them; `db()` and the
//! other helpers are that module's.

use super::*;

#[test]
fn a_duplicate_keeps_what_was_held_on_the_original() {
    let conn = db();
    merge_server(&conn, "u1", "curl x | sh", true, L8080);
    let spec: TunnelSpec = serde_json::from_str::<Vec<TunnelSpec>>(L8080).unwrap().remove(0);
    // What clone_server copies: the commands and forwards, not autostart.
    conn.execute(
        "INSERT INTO servers(name, tunnels, autostart, run_on_connect, uuid) SELECT name, tunnels, 0, run_on_connect, 'u2' FROM servers WHERE uuid = 'u1'",
        [],
    )
    .unwrap();
    copy_held(&conn, 1, 2).unwrap();
    let held = held_for(&conn, "u2");
    assert!(held.contains(RUN_ON_CONNECT));
    assert!(held.contains(&tunnel_item(&spec)));
    assert!(!held.contains(AUTOSTART));
    // The original's own entries stay, and allowing the copy's leaves them.
    allow(&conn, "u2", &[]).unwrap();
    assert!(held_for(&conn, "u1").contains(RUN_ON_CONNECT));
}

#[test]
fn a_duplicate_of_an_allowed_server_holds_nothing() {
    let conn = db();
    merge_server(&conn, "u1", "ls", false, L8080);
    allow(&conn, "u1", &[]).unwrap();
    conn.execute("INSERT INTO servers(name, uuid) VALUES ('c', 'u2')", []).unwrap();
    copy_held(&conn, 1, 2).unwrap();
    assert!(held_for(&conn, "u2").is_empty());
}

#[test]
fn allowing_commands_counts_only_for_the_text_that_was_shown() {
    let conn = db();
    merge_server(&conn, "u1", "uptime", false, "[]");
    // A newer synced text while the first is held keeps the same item.
    merge_server(&conn, "u1", "curl x | sh", false, "[]");
    assert!(held_for(&conn, "u1").contains(RUN_ON_CONNECT));
    // An OK for the text shown before is refused; nothing is let go.
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("uptime"), None), Err(HELD_CHANGED.to_string()));
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, None, None), Err(HELD_CHANGED.to_string()));
    assert!(held_for(&conn, "u1").contains(RUN_ON_CONNECT));
    // Shown again, it can be allowed.
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("curl x | sh"), None), Ok(()));
    // Not allowing them needs nothing; a stale text is refused all the same.
    assert_eq!(check_seen_run_on_connect(&conn, "u1", false, None, None), Ok(()));
    assert!(check_seen_run_on_connect(&conn, "u1", false, Some("uptime"), None).is_err());
}

/// "Each new terminal" turned on by sync while the question about the
/// same text was on screen without it: that OK doesn't cover it.
#[test]
fn allowing_commands_counts_only_for_the_terminals_that_were_shown() {
    let conn = db();
    merge_server(&conn, "u1", "uptime", false, "[]");
    merge_every(&conn, "u1", true);
    assert!(held_for(&conn, "u1").contains(RUN_ON_CONNECT));
    // Shown as for the first terminal only (or not said): refused.
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("uptime"), Some(false)), Err(HELD_CHANGED.to_string()));
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("uptime"), None), Err(HELD_CHANGED.to_string()));
    // Shown as they are: allowed.
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("uptime"), Some(true)), Ok(()));
    // And the other way: shown as for each terminal, now for the first only.
    merge_every(&conn, "u1", false);
    assert_eq!(check_seen_run_on_connect(&conn, "u1", true, Some("uptime"), Some(true)), Err(HELD_CHANGED.to_string()));
}

#[test]
fn a_forward_stopped_here_and_started_again_by_sync_is_held() {
    let conn = db();
    conn.execute("INSERT INTO servers(name, tunnels, uuid) VALUES('s', ?1, 'u1')", [L8080]).unwrap();
    // Stopped on this device, and synced that way.
    let stopped = r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432","stopped":true}]"#;
    conn.execute("UPDATE servers SET tunnels=?1 WHERE uuid='u1'", [stopped]).unwrap();
    // Someone else's record starts it again (and names it): held.
    merge_server(&conn, "u1", "", false, r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432","name":"db"}]"#);
    assert_eq!(held_list(&conn, "u1"), vec!["tunnel:L|127.0.0.1:8080|db:5432".to_string()]);
    let mut specs: Vec<TunnelSpec> = serde_json::from_str(L8080).unwrap();
    assert_eq!(hold_tunnels(&conn, Some("u1"), &mut specs), 1, "it starts stopped here");
    // Renaming it there keeps it held; stopping it there lets it go.
    merge_server(&conn, "u1", "", false, r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432","name":"x"}]"#);
    assert!(!held_for(&conn, "u1").is_empty());
    merge_server(&conn, "u1", "", false, stopped);
    assert!(held_for(&conn, "u1").is_empty());
    // A new forward that comes in stopped runs nothing: not held.
    merge_server(&conn, "u1", "", false, r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432","stopped":true},{"type":"D","local":"0.0.0.0:1080","stopped":true}]"#);
    assert!(held_for(&conn, "u1").is_empty());
}

/// What the merge engine does for a server record that sets `protocol`
/// and `port` (None: an older peer's record without the `protocol` key,
/// which leaves the row's): a new row when there's none.
fn merge_switch(conn: &Connection, uuid: &str, protocol: Option<Option<&str>>, port: i64) {
    merging(conn, true);
    let before = before_merge(conn, "servers", uuid).unwrap();
    let updated = match protocol {
        Some(p) => conn.execute("UPDATE servers SET protocol=?1, port=?2 WHERE uuid=?3", params![p, port, uuid]).unwrap(),
        None => conn.execute("UPDATE servers SET port=?1 WHERE uuid=?2", params![port, uuid]).unwrap(),
    };
    if updated == 0 {
        conn.execute(
            "INSERT INTO servers(name, protocol, port, uuid) VALUES('s', ?1, ?2, ?3)",
            params![protocol.flatten(), port, uuid],
        )
        .unwrap();
    }
    hold_changes(conn, "servers", uuid, before.as_ref()).unwrap();
    merging(conn, false);
}

fn merge_protocol(conn: &Connection, uuid: &str, protocol: Option<&str>) {
    let port = match protocol {
        Some("vnc") => 5900,
        Some("telnet") => 23,
        Some("rdp") => 3389,
        _ => 22,
    };
    merge_switch(conn, uuid, Some(protocol), port);
}

/// A record whose stamp loses to this device's: the row isn't written.
fn merge_skipped(conn: &Connection, uuid: &str) {
    merging(conn, true);
    let before = before_merge(conn, "servers", uuid).unwrap();
    hold_changes(conn, "servers", uuid, before.as_ref()).unwrap();
    merging(conn, false);
}

fn protocol_of(conn: &Connection, uuid: &str) -> Option<String> {
    conn.query_row("SELECT protocol FROM servers WHERE uuid=?1", [uuid], |r| r.get(0)).unwrap()
}

fn port_of(conn: &Connection, uuid: &str) -> i64 {
    conn.query_row("SELECT port FROM servers WHERE uuid=?1", [uuid], |r| r.get(0)).unwrap()
}

fn held_list(conn: &Connection, uuid: &str) -> Vec<String> {
    held_for(conn, uuid).into_iter().collect()
}

/// The server's held switches. A switch is a move too (`hold_moved`),
/// which the switch tests don't look at.
fn held_switches(conn: &Connection, uuid: &str) -> Vec<String> {
    held_for(conn, uuid).into_iter().filter(|i| i.starts_with(PROTOCOL_PREFIX)).collect()
}

#[test]
fn a_synced_switch_from_ssh_to_telnet_or_vnc_is_held_with_the_old_protocol_and_port_kept() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("vnc"));
    assert_eq!((protocol_of(&conn, "u1"), port_of(&conn, "u1")), (None, 22), "still SSH on 22 here");
    assert_eq!(held_switch(&conn, "u1"), Some(HeldSwitch { protocol: "vnc".into(), port: Some(5900) }));
    // A later record that still says VNC (or one that loses LWW) keeps it held.
    merge_protocol(&conn, "u1", Some("vnc"));
    merge_skipped(&conn, "u1");
    assert_eq!((protocol_of(&conn, "u1"), port_of(&conn, "u1")), (None, 22));
    assert_eq!(held_switches(&conn, "u1"), vec!["protocol:vnc|5900".to_string()]);
    // Switched on to telnet: the held switch is to telnet now.
    merge_protocol(&conn, "u1", Some("telnet"));
    assert_eq!(held_switches(&conn, "u1"), vec!["protocol:telnet|23".to_string()]);
    assert_eq!((protocol_of(&conn, "u1"), port_of(&conn, "u1")), (None, 22));
    // Back to SSH: nothing to hold.
    merge_protocol(&conn, "u1", None);
    assert!(held_for(&conn, "u1").is_empty());
    assert_eq!(protocol_of(&conn, "u1"), None);
}

#[test]
fn an_older_peers_record_without_the_protocol_keeps_the_hold() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("vnc"));
    // A peer from before VNC edits the server: its record has no protocol.
    merge_switch(&conn, "u1", None, 5900);
    assert_eq!((protocol_of(&conn, "u1"), port_of(&conn, "u1")), (None, 22));
    assert_eq!(held_switches(&conn, "u1"), vec!["protocol:vnc|5900".to_string()]);
}

#[test]
fn a_literal_ssh_is_kept_as_plain_ssh() {
    // A peer that wrote "ssh" rather than NULL: kept as NULL, as the form
    // saves SSH, so saving the form doesn't count as choosing.
    let conn = db();
    merge_protocol(&conn, "u1", None);
    conn.execute("UPDATE servers SET protocol='ssh' WHERE uuid='u1'", []).unwrap();
    merge_protocol(&conn, "u1", Some("telnet"));
    assert_eq!(protocol_of(&conn, "u1"), None);
    assert_eq!(held_protocol(&conn, "u1").as_deref(), Some("telnet"));
    conn.execute("UPDATE servers SET protocol=NULL WHERE uuid='u1'", []).unwrap();
    assert_eq!(held_protocol(&conn, "u1").as_deref(), Some("telnet"));
}

#[test]
fn a_new_server_rdp_and_a_switch_between_others_are_not_held() {
    let conn = db();
    merge_protocol(&conn, "u1", Some("vnc"));
    assert_eq!(protocol_of(&conn, "u1").as_deref(), Some("vnc"), "a new server comes as it is");
    merge_protocol(&conn, "u1", Some("telnet"));
    assert_eq!(protocol_of(&conn, "u1").as_deref(), Some("telnet"), "it wasn't an SSH server");
    merge_protocol(&conn, "u2", None);
    merge_protocol(&conn, "u2", Some("rdp"));
    assert_eq!(protocol_of(&conn, "u2").as_deref(), Some("rdp"));
    assert!(held_switches(&conn, "u1").is_empty() && held_switches(&conn, "u2").is_empty(), "no switch is held");
}

#[test]
fn a_switch_to_telnet_or_vnc_by_way_of_rdp_is_held_too() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("rdp"));
    assert_eq!(protocol_of(&conn, "u1").as_deref(), Some("rdp"), "RDP isn't held");
    merge_protocol(&conn, "u1", Some("vnc"));
    assert_eq!((protocol_of(&conn, "u1").as_deref(), port_of(&conn, "u1")), (Some("rdp"), 3389), "still RDP on 3389 here");
    assert_eq!(held_switches(&conn, "u1"), vec!["protocol:vnc|5900".to_string()]);
    // Keeping it keeps it RDP, as this device's own edit.
    assert!(keep_ssh(&conn, "u1").unwrap());
    assert_eq!((protocol_of(&conn, "u1").as_deref(), port_of(&conn, "u1")), (Some("rdp"), 3389));
    assert!(held_switches(&conn, "u1").is_empty());
    // Allowed instead, the switch is made.
    merge_protocol(&conn, "u2", None);
    merge_protocol(&conn, "u2", Some("rdp"));
    merge_protocol(&conn, "u2", Some("telnet"));
    allow(&conn, "u2", &[]).unwrap();
    assert_eq!((protocol_of(&conn, "u2").as_deref(), port_of(&conn, "u2")), (Some("telnet"), 23));
}

#[test]
fn allowing_a_held_switch_makes_it_with_its_port_and_the_users_own_change_releases_it() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("vnc"));
    // Allowing another item leaves it.
    allow(&conn, "u1", &["protocol:telnet|23".to_string()]).unwrap();
    assert_eq!(protocol_of(&conn, "u1"), None);
    allow(&conn, "u1", &["protocol:vnc|5900".to_string()]).unwrap();
    assert_eq!((protocol_of(&conn, "u1").as_deref(), port_of(&conn, "u1")), (Some("vnc"), 5900));
    assert!(held_for(&conn, "u1").is_empty());
    let merge_flag: i64 = conn.query_row("SELECT val FROM sync_flags WHERE key='merge'", [], |r| r.get(0)).unwrap();
    assert_eq!(merge_flag, 0, "the merge flag is off again");

    merge_protocol(&conn, "u2", None);
    merge_protocol(&conn, "u2", Some("telnet"));
    // Saving the same protocol back (the form does) is not an OK.
    conn.execute("UPDATE servers SET protocol=NULL WHERE uuid='u2'", []).unwrap();
    assert!(held_protocol(&conn, "u2").is_some());
    // Choosing one on this device is.
    conn.execute("UPDATE servers SET protocol='rdp' WHERE uuid='u2'", []).unwrap();
    assert!(held_for(&conn, "u2").is_empty());
    assert_eq!(protocol_of(&conn, "u2").as_deref(), Some("rdp"));
}

#[test]
fn keep_it_ssh_drops_the_switch_and_keeps_the_old_port() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("vnc"));
    assert!(keep_ssh(&conn, "u1").unwrap());
    assert!(held_for(&conn, "u1").is_empty());
    assert_eq!((protocol_of(&conn, "u1"), port_of(&conn, "u1")), (None, 22));
    assert!(!keep_ssh(&conn, "u1").unwrap(), "nothing left to keep");
}

#[test]
fn a_duplicate_doesnt_take_a_held_switch_of_protocol() {
    let conn = db();
    merge_protocol(&conn, "u1", None);
    merge_protocol(&conn, "u1", Some("vnc"));
    conn.execute("INSERT INTO servers(name, protocol, port, uuid) SELECT name, protocol, port, 'u2' FROM servers WHERE uuid = 'u1'", []).unwrap();
    copy_held(&conn, 1, 2).unwrap();
    assert_eq!((protocol_of(&conn, "u2"), port_of(&conn, "u2")), (None, 22));
    assert!(held_for(&conn, "u2").is_empty());
    assert!(held_protocol(&conn, "u1").is_some(), "the original stays held");
}

/// In the fork, a switch to telnet or VNC is held as a switch: the row
/// stays SSH here (`hold_switch`), so it goes nowhere new yet.
#[test]
fn a_switch_to_telnet_is_held_as_a_switch_not_a_move() {
    let conn = with_allowed_server();
    merge_set(&conn, "u1", "protocol='telnet'");
    assert!(held_for(&conn, "u1").iter().any(|i| i.starts_with(PROTOCOL_PREFIX)));
    assert_eq!(target_words(&conn, "u1").as_deref(), Some("old.example:22"));
}

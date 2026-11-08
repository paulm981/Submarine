//! The fork's own tests in `sync_engine_tests`, kept in a file of their own (#136) so
//! a sync with the original project doesn't conflict on them. A child of
//! `sync_engine_tests`, so they use its helpers as they are.

use super::*;

// A co-member's move that lets the AI do more on a server here (into a
// folder this device set to "Run anything", or a folder moved there) is
// held until this device's user allows it; one that changes nothing for
// the AI isn't, and neither is anything in a personal profile.
#[test]
fn a_shared_profile_holds_a_move_that_loosens_the_ai_s_level() {
    let (a, _ha) = device("A");
    a.execute_batch(
        "INSERT INTO folders(name) VALUES('Prod'), ('Lab'), ('Quiet');
         INSERT INTO servers(name,host,port,folder_id) VALUES('db-01','h',22,(SELECT id FROM folders WHERE name='Prod'));",
    )
    .unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    ai::trust::create_schema(&b, "this-install-0000").unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let id_of = |c: &Connection, t: &str, n: &str| -> i64 { c.query_row(&format!("SELECT id FROM {t} WHERE name=?1"), [n], |r| r.get(0)).unwrap() };
    let (db, lab) = (id_of(&b, "servers", "db-01"), id_of(&b, "folders", "Lab"));
    let uuid: String = b.query_row("SELECT uuid FROM servers WHERE id=?1", [db], |r| r.get(0)).unwrap();
    ai::trust::set_level(&b, "folder", lab, Some(3)).unwrap();
    let held = |b: &Connection| -> Vec<String> { approvals::held_for(b, &uuid).into_iter().collect() };
    let sync = |a: &Connection, b: &Connection| apply_remote_records(b, &KEY, &collect_local_records(a, &KEY, "").unwrap(), &hb).unwrap();

    // Into a folder nobody set anything on: nothing to hold.
    a.execute("UPDATE servers SET folder_id=(SELECT id FROM folders WHERE name='Quiet') WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    assert!(held(&b).is_empty());
    // Into Lab, at "Run anything" here: held, and every command asks.
    a.execute("UPDATE servers SET folder_id=(SELECT id FROM folders WHERE name='Lab') WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    let items = held(&b);
    assert_eq!(items.len(), 1, "{items:?}");
    let h: ai::trust::HeldPolicy = serde_json::from_str(&items[0][ai::trust::HELD_ITEM.len()..]).unwrap();
    assert_eq!((h.level, h.moved.as_str(), h.moved_folder, h.into.as_str()), (3, "db-01", false, "Lab"));
    let p = ai::trust::policy_for(&b, Some(db));
    assert_eq!(p.level, 1);
    assert!(p.force_ask.is_some());
    // Allowed, it's what Lab says.
    approvals::allow(&b, &uuid, &[]).unwrap();
    assert_eq!(ai::trust::policy_for(&b, Some(db)), ai::trust::Policy { level: 3, ..ai::trust::policy_for(&b, Some(db)) });
    assert!(ai::trust::policy_for(&b, Some(db)).force_ask.is_none());

    // Out again, then its old folder moved into Lab: held, naming the
    // folder.
    a.execute("UPDATE servers SET folder_id=(SELECT id FROM folders WHERE name='Prod') WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    assert!(held(&b).is_empty(), "a move that lets the AI do less isn't held");
    a.execute("UPDATE folders SET parent_id=(SELECT id FROM folders WHERE name='Lab') WHERE name='Prod'", []).unwrap();
    sync(&a, &b);
    let h: ai::trust::HeldPolicy = serde_json::from_str(&held(&b)[0][ai::trust::HELD_ITEM.len()..]).unwrap();
    assert_eq!((h.moved.as_str(), h.moved_folder, h.into.as_str()), ("Prod", true, "Lab"));
    // Kept as it was: no longer held, and "Ask every time" here.
    ai::trust::keep_as_it_was(&b, db).unwrap();
    assert!(held(&b).is_empty());
    let p = ai::trust::policy_for(&b, Some(db));
    assert_eq!((p.level, p.force_ask.is_none()), (1, true));

    // A personal profile holds nothing.
    let (c, hc) = device("C");
    approvals::create_schema(&c).unwrap();
    ai::trust::create_schema(&c, "this-install-0000").unwrap();
    a.execute("UPDATE folders SET parent_id=NULL WHERE name='Prod'", []).unwrap();
    apply_remote_records(&c, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hc).unwrap();
    ai::trust::set_level(&c, "folder", id_of(&c, "folders", "Lab"), Some(3)).unwrap();
    a.execute("UPDATE servers SET folder_id=(SELECT id FROM folders WHERE name='Lab') WHERE name='db-01'", []).unwrap();
    apply_remote_records(&c, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hc).unwrap();
    assert!(approvals::held_by_server(&c).is_empty());
}

// A server new here that lands where the AI may do more than by default
// is held; one somewhere plain isn't. A server the AI may run commands
// on without asking is held when someone points it elsewhere.
#[test]
fn a_shared_profile_holds_new_and_re_pointed_servers() {
    let (a, _ha) = device("A");
    a.execute_batch(
        "INSERT INTO folders(name) VALUES('Lab'), ('Quiet');
         INSERT INTO servers(name,host,port,username) VALUES('db-01','db.example',22,'deploy'), ('web-01','web.example',22,'deploy');",
    )
    .unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    ai::trust::create_schema(&b, "this-install-0000").unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    let sync = |a: &Connection, b: &Connection| apply_remote_records(b, &KEY, &collect_local_records(a, &KEY, "").unwrap(), &hb).unwrap();
    sync(&a, &b);
    let id_of = |c: &Connection, t: &str, n: &str| -> i64 { c.query_row(&format!("SELECT id FROM {t} WHERE name=?1"), [n], |r| r.get(0)).unwrap() };
    let held_of = |b: &Connection, name: &str| -> Option<ai::trust::HeldPolicy> {
        let uuid: String = b.query_row("SELECT uuid FROM servers WHERE name=?1", [name], |r| r.get(0)).unwrap();
        approvals::held_for(b, &uuid)
            .into_iter()
            .find(|i| i.starts_with(ai::trust::HELD_ITEM))
            .map(|i| serde_json::from_str(&i[ai::trust::HELD_ITEM.len()..]).unwrap())
    };
    ai::trust::set_level(&b, "folder", id_of(&b, "folders", "Lab"), Some(3)).unwrap();
    ai::trust::set_level(&b, "server", id_of(&b, "servers", "db-01"), Some(2)).unwrap();

    // New in Lab: held. New in Quiet: not.
    a.execute_batch(
        "INSERT INTO servers(name,host,port,folder_id) VALUES('lab-01','lab.example',22,(SELECT id FROM folders WHERE name='Lab'));
         INSERT INTO servers(name,host,port,folder_id) VALUES('q-01','q.example',22,(SELECT id FROM folders WHERE name='Quiet'));",
    )
    .unwrap();
    sync(&a, &b);
    let h = held_of(&b, "lab-01").expect("held");
    assert_eq!((h.change.as_str(), h.level, h.moved.as_str(), h.into.as_str()), ("added", 3, "lab-01", "Lab"));
    assert_eq!(ai::trust::policy_for(&b, Some(id_of(&b, "servers", "lab-01"))).level, 1);
    assert!(held_of(&b, "q-01").is_none());

    // db-01 runs allowed commands here: pointed elsewhere, it's held.
    // web-01 asks every time: pointed elsewhere, it isn't. A change to
    // anything else isn't either.
    a.execute("UPDATE servers SET notes='x' WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    assert!(held_of(&b, "db-01").is_none());
    a.execute("UPDATE servers SET host='evil.example' WHERE name IN ('db-01','web-01')", []).unwrap();
    sync(&a, &b);
    let h = held_of(&b, "db-01").expect("held");
    assert_eq!((h.change.as_str(), h.level), ("pointed", 2));
    assert!(ai::trust::policy_for(&b, Some(id_of(&b, "servers", "db-01"))).force_ask.is_some());
    assert!(held_of(&b, "web-01").is_none());
    approvals::allow(&b, &b.query_row("SELECT uuid FROM servers WHERE name='db-01'", [], |r| r.get::<_, String>(0)).unwrap(), &[]).unwrap();
    a.execute("UPDATE servers SET username='root' WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    assert_eq!(held_of(&b, "db-01").map(|h| h.change), Some("pointed".into()), "its user too");
    approvals::allow(&b, &b.query_row("SELECT uuid FROM servers WHERE name='db-01'", [], |r| r.get::<_, String>(0)).unwrap(), &[]).unwrap();
    a.execute("UPDATE servers SET proxy_type='socks5', proxy_host='proxy.example', proxy_port=1080 WHERE name='db-01'", []).unwrap();
    sync(&a, &b);
    assert_eq!(held_of(&b, "db-01").map(|h| h.change), Some("pointed".into()), "its proxy too");
}

// A careful server deleted and made again under a default that runs
// anything comes back as a new server: held, not let through.
#[test]
fn a_careful_server_made_again_under_a_loose_default_is_held() {
    let (a, _ha) = device("A");
    a.execute("INSERT INTO servers(name,host,port) VALUES('prod-db','db.example',22)", []).unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    ai::trust::create_schema(&b, "this-install-0000").unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    let sync = |a: &Connection, b: &Connection| apply_remote_records(b, &KEY, &collect_local_records(a, &KEY, "").unwrap(), &hb).unwrap();
    sync(&a, &b);
    let id: i64 = b.query_row("SELECT id FROM servers WHERE name='prod-db'", [], |r| r.get(0)).unwrap();
    ai::trust::set_level(&b, "default", 0, Some(3)).unwrap();
    ai::trust::set_level(&b, "server", id, Some(1)).unwrap();
    a.execute("DELETE FROM servers WHERE name='prod-db'", []).unwrap();
    a.execute("INSERT INTO servers(name,host,port) VALUES('prod-db','db.example',22)", []).unwrap();
    sync(&a, &b);
    let (id, uuid): (i64, String) = b.query_row("SELECT id, uuid FROM servers WHERE name='prod-db'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let items: Vec<String> = approvals::held_for(&b, &uuid).into_iter().collect();
    assert_eq!(items.len(), 1, "{items:?}");
    let h: ai::trust::HeldPolicy = serde_json::from_str(&items[0][ai::trust::HELD_ITEM.len()..]).unwrap();
    assert_eq!((h.change.as_str(), h.level, h.into.as_str()), ("added", 3, ""));
    let p = ai::trust::policy_for(&b, Some(id));
    assert_eq!((p.level, p.force_ask.is_some()), (1, true));
}

// A move out of a folder set to "Ask every time", where every server
// runs anything, is held too.
#[test]
fn a_move_out_of_a_careful_folder_is_held() {
    let (a, _ha) = device("A");
    a.execute_batch(
        "INSERT INTO folders(name) VALUES('Prod');
         INSERT INTO servers(name,host,port,folder_id) VALUES('db-01','h',22,(SELECT id FROM folders WHERE name='Prod'));",
    )
    .unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    ai::trust::create_schema(&b, "this-install-0000").unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let prod: i64 = b.query_row("SELECT id FROM folders WHERE name='Prod'", [], |r| r.get(0)).unwrap();
    ai::trust::set_level(&b, "default", 0, Some(3)).unwrap();
    ai::trust::set_level(&b, "folder", prod, Some(1)).unwrap();
    a.execute("UPDATE servers SET folder_id=NULL WHERE name='db-01'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let uuid: String = b.query_row("SELECT uuid FROM servers WHERE name='db-01'", [], |r| r.get(0)).unwrap();
    let items: Vec<String> = approvals::held_for(&b, &uuid).into_iter().collect();
    assert_eq!(items.len(), 1);
    let h: ai::trust::HeldPolicy = serde_json::from_str(&items[0][ai::trust::HELD_ITEM.len()..]).unwrap();
    assert_eq!((h.level, h.into.as_str()), (3, ""), "to the top level");
}

// A forward a co-member adds while this device has a session open on the
// server: the session's next save keeps it (and its hold) instead of
// writing the list it read before over it.
#[test]
fn a_sessions_save_keeps_a_forward_that_came_by_sync() {
    let l8080 = r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432"}]"#;
    let (a, ha) = device("A");
    a.execute("INSERT INTO servers(name,host,port,tunnels) VALUES('web','h',22,?1)", [l8080]).unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let (id, uuid): (i32, String) = b.query_row("SELECT id, uuid FROM servers WHERE name='web'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    approvals::allow(&b, &uuid, &[]).unwrap();
    assert!(take_synced_tunnels_base(&b, id).is_none(), "a new server has no session to be out of date");
    // B's session reads the rules; then A adds a SOCKS forward.
    let ours: Vec<tunnel::TunnelSpec> = serde_json::from_str(l8080).unwrap();
    a.execute(
        "UPDATE servers SET tunnels=?1 WHERE name='web'",
        [r#"[{"type":"L","local":"127.0.0.1:8080","remote":"db:5432"},{"type":"D","local":"0.0.0.0:1080"}]"#],
    )
    .unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert!(approvals::held_for(&b, &uuid).contains("tunnel:D|0.0.0.0:1080|"));
    // B stops its forward and saves, as persist_session_tunnels does.
    let mut ours = ours;
    let first = ours[0].clone();
    tunnel::rule_stopped(&mut ours, &first);
    let base = take_synced_tunnels_base(&b, id).expect("sync changed the saved forwards");
    let saved: Vec<tunnel::TunnelSpec> =
        serde_json::from_str(&b.query_row("SELECT tunnels FROM servers WHERE id=?1", [id], |r| r.get::<_, String>(0)).unwrap()).unwrap();
    let merged = tunnel::merge_rules(&base, &ours, &saved);
    b.execute("UPDATE servers SET tunnels=?1 WHERE id=?2", rusqlite::params![serde_json::to_string(&merged).unwrap(), id]).unwrap();
    let now: Vec<tunnel::TunnelSpec> =
        serde_json::from_str(&b.query_row("SELECT tunnels FROM servers WHERE id=?1", [id], |r| r.get::<_, String>(0)).unwrap()).unwrap();
    assert_eq!(now.len(), 2, "the synced forward stays");
    assert!(now[0].stopped && !now[1].stopped);
    assert!(approvals::held_for(&b, &uuid).contains("tunnel:D|0.0.0.0:1080|"), "and stays held");
    assert!(take_synced_tunnels_base(&b, id).is_none(), "taken once");
    // A record that leaves the forwards alone marks nothing.
    apply_remote_records(&a, &KEY, &collect_local_records(&b, &KEY, "").unwrap(), &ha).unwrap();
    a.execute("UPDATE servers SET host='h2' WHERE name='web'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert!(take_synced_tunnels_base(&b, id).is_none());
}

/// A shared device B with A's SSH server "web" on port 22, and A having
/// switched it to VNC on 5900 since: B holds the switch.
fn b_holds_a_switch_to_vnc() -> (Connection, std::sync::Arc<hlc::Hlc>, Connection, std::sync::Arc<hlc::Hlc>, String) {
    let (a, ha) = device("A");
    a.execute("INSERT INTO servers(name,host,port) VALUES('web','h',22)", []).unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let uuid: String = b.query_row("SELECT uuid FROM servers WHERE name='web'", [], |r| r.get(0)).unwrap();
    assert!(approvals::held_for(&b, &uuid).is_empty(), "an SSH server arrives as it is");
    a.execute("UPDATE servers SET protocol='vnc', port=5900 WHERE name='web'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    (a, ha, b, hb, uuid)
}

/// The protocol and port of the server named `name`.
fn proto_port(c: &Connection, name: &str) -> (Option<String>, i64) {
    c.query_row("SELECT protocol, port FROM servers WHERE name=?1", [name], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
}

/// What `c` would push for the server named `name`: its protocol and port.
fn pushed(c: &Connection, name: &str) -> (serde_json::Value, serde_json::Value) {
    let uuid: String = c.query_row("SELECT uuid FROM servers WHERE name=?1", [name], |r| r.get(0)).unwrap();
    let recs = collect_local_records(c, &KEY, "").unwrap();
    let srv = recs.iter().find(|r| r.entity_type == "servers" && r.uuid == uuid).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&decrypt_entity(srv.blob.as_ref().unwrap(), &KEY).unwrap()).unwrap();
    (v["protocol"].clone(), v["port"].clone())
}

// A co-member switching an SSH server to VNC (or telnet) is held: this
// device keeps it an SSH server on its old port until its user allows the
// switch, and an edit made here meanwhile doesn't switch it back for
// everyone else.
#[test]
fn a_shared_profile_holds_a_switch_from_ssh_to_vnc() {
    let (a, ha, b, hb, uuid) = b_holds_a_switch_to_vnc();
    assert_eq!(proto_port(&b, "web"), (None, 22), "this device keeps it SSH on 22");
    assert_eq!(approvals::held_for(&b, &uuid).into_iter().collect::<Vec<_>>(), vec!["protocol:vnc|5900".to_string()]);

    // An edit here pushes the synced protocol and port, not this device's.
    b.execute("UPDATE servers SET name='web2' WHERE name='web'", []).unwrap();
    assert_eq!(pushed(&b, "web2"), (serde_json::json!("vnc"), serde_json::json!(5900)));
    apply_remote_records(&a, &KEY, &collect_local_records(&b, &KEY, "").unwrap(), &ha).unwrap();
    assert_eq!(proto_port(&a, "web2"), (Some("vnc".into()), 5900), "the other device's switch stays");

    // The other device's next edit, which still says VNC, stays held.
    a.execute("UPDATE servers SET host='h2' WHERE name='web2'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let host: String = b.query_row("SELECT host FROM servers WHERE name='web2'", [], |r| r.get(0)).unwrap();
    assert_eq!((host.as_str(), proto_port(&b, "web2")), ("h2", (None, 22)));

    // Allowed: now a VNC server on 5900 here too.
    approvals::allow(&b, &uuid, &["protocol:vnc|5900".to_string()]).unwrap();
    assert_eq!(proto_port(&b, "web2"), (Some("vnc".into()), 5900));
    // What's left is the move to h2, a question of its own.
    let held: Vec<String> = approvals::held_for(&b, &uuid).into_iter().collect();
    assert!(held.iter().all(|i| i.starts_with(approvals::MOVED)), "{held:?}");
}

// An older peer's record has no `protocol` key: the hold stays, and the
// row stays SSH on its old port.
#[test]
fn an_older_peers_edit_keeps_a_held_switch() {
    let (a, _ha, b, hb, uuid) = b_holds_a_switch_to_vnc();
    a.execute("UPDATE servers SET host='h2' WHERE name='web'", []).unwrap();
    let ahead = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 + 60_000;
    let mut recs = collect_local_records(&a, &KEY, "").unwrap();
    for r in recs.iter_mut().filter(|r| r.entity_type == "servers") {
        let mut obj: serde_json::Value = serde_json::from_slice(&decrypt_entity(r.blob.as_deref().unwrap(), &KEY).unwrap()).unwrap();
        obj.as_object_mut().unwrap().remove("protocol");
        r.blob = Some(encrypt_entity(&serde_json::to_vec(&obj).unwrap(), &KEY).unwrap());
        r.updated_at = format!("{ahead:015}:00000:zzz");
    }
    apply_remote_records(&b, &KEY, &recs, &hb).unwrap();
    let host: String = b.query_row("SELECT host FROM servers WHERE name='web'", [], |r| r.get(0)).unwrap();
    assert_eq!((host.as_str(), proto_port(&b, "web")), ("h2", (None, 22)));
    assert_eq!(approvals::held_protocol(&b, &uuid).as_deref(), Some("vnc"));
}

// Leaving the share keeps what was held, and later syncs between the
// user's own computers keep a held switch held.
#[test]
fn a_held_switch_stays_held_after_leaving_the_share() {
    let (a, _ha, b, hb, uuid) = b_holds_a_switch_to_vnc();
    b.execute("DELETE FROM sync_meta WHERE key='share_id'", []).unwrap();
    a.execute("UPDATE servers SET host='h2' WHERE name='web'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let host: String = b.query_row("SELECT host FROM servers WHERE name='web'", [], |r| r.get(0)).unwrap();
    assert_eq!((host.as_str(), proto_port(&b, "web")), ("h2", (None, 22)));
    assert_eq!(approvals::held_protocol(&b, &uuid).as_deref(), Some("vnc"));
    // Switched back to SSH over there: nothing held any more.
    a.execute("UPDATE servers SET protocol=NULL, port=22 WHERE name='web'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert!(approvals::held_for(&b, &uuid).is_empty());
    assert_eq!(proto_port(&b, "web"), (None, 22));
}

// A peer that wrote a literal "ssh": the server is kept as plain SSH
// (NULL), so saving the form, which saves SSH as NULL, isn't a choice
// that releases the hold and pushes SSH.
#[test]
fn a_literal_ssh_is_kept_as_null() {
    let (a, _ha) = device("A");
    a.execute("INSERT INTO servers(name,host,port,protocol) VALUES('web','h',22,'ssh')", []).unwrap();
    let (b, hb) = device("B");
    approvals::create_schema(&b).unwrap();
    b.execute("INSERT INTO sync_meta(key,value) VALUES('share_id','s1')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let uuid: String = b.query_row("SELECT uuid FROM servers WHERE name='web'", [], |r| r.get(0)).unwrap();
    a.execute("UPDATE servers SET protocol='telnet', port=23 WHERE name='web'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert_eq!(proto_port(&b, "web"), (None, 22));
    // What the form's save does for an SSH server: no change, still held.
    b.execute("UPDATE servers SET protocol=NULL WHERE name='web' AND protocol IS NOT NULL", []).unwrap();
    assert_eq!(approvals::held_protocol(&b, &uuid).as_deref(), Some("telnet"));
    assert_eq!(pushed(&b, "web"), (serde_json::json!("telnet"), serde_json::json!(23)));
}

// Keep it SSH: this device's own edit, which takes the server back to SSH
// on its old port everywhere.
#[test]
fn keep_it_ssh_goes_to_everyone() {
    let (a, ha, b, _hb, uuid) = b_holds_a_switch_to_vnc();
    assert!(approvals::keep_ssh(&b, &uuid).unwrap());
    assert!(approvals::held_for(&b, &uuid).is_empty());
    assert_eq!(pushed(&b, "web"), (serde_json::Value::Null, serde_json::json!(22)));
    apply_remote_records(&a, &KEY, &collect_local_records(&b, &KEY, "").unwrap(), &ha).unwrap();
    assert_eq!(proto_port(&a, "web"), (None, 22));
}

// A copy of a held server is the SSH server its maker sees: nothing held
// on it, and it syncs as SSH, so nobody gets an unheld VNC copy.
#[test]
fn a_duplicate_of_a_held_server_syncs_as_ssh() {
    let (_a, _ha, b, _hb, uuid) = b_holds_a_switch_to_vnc();
    let id: i32 = b.query_row("SELECT id FROM servers WHERE name='web'", [], |r| r.get(0)).unwrap();
    clone_server_row(&b, id).unwrap();
    let copy: String = b.query_row("SELECT uuid FROM servers WHERE name='web (copy)'", [], |r| r.get(0)).unwrap();
    assert!(approvals::held_for(&b, &copy).is_empty());
    assert_eq!(proto_port(&b, "web (copy)"), (None, 22));
    assert_eq!(pushed(&b, "web (copy)"), (serde_json::Value::Null, serde_json::json!(22)));
    assert_eq!(approvals::held_protocol(&b, &uuid).as_deref(), Some("vnc"), "the original stays held");
}

#[test]
fn scheme_ids_are_cleaned() {
    assert_eq!(clean_scheme_id(" tokyo-night "), Some("tokyo-night".into()));
    assert_eq!(clean_scheme_id("Nord"), Some("nord".into()));
    assert_eq!(clean_scheme_id("  "), None);
    assert_eq!(clean_scheme_id("<x>;"), Some("x".into()));
    assert_eq!(clean_scheme_id(&"a".repeat(100)).unwrap().len(), 64);
}

// A server's terminal scheme syncs like its other fields — and an edit
// from a peer on a build without the column (the key absent from its
// record) changes what it changed but keeps the scheme, both on a row
// that exists and on one it inserts (NULL there).
#[test]
fn terminal_theme_syncs_and_an_older_peer_keeps_it() {
    let (a, ha) = device("A");
    a.execute("INSERT INTO servers(name, host, port, terminal_theme) VALUES('prod','h1',22,'nord')", []).unwrap();
    let (b, hb) = device("B");
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let theme: Option<String> = b.query_row("SELECT terminal_theme FROM servers WHERE name='prod'", [], |r| r.get(0)).unwrap();
    assert_eq!(theme.as_deref(), Some("nord"), "the scheme must sync");

    // An older peer edits the host: its record has no terminal_theme key.
    a.execute("UPDATE servers SET host='h2' WHERE name='prod'", []).unwrap();
    let mut recs = collect_local_records(&a, &KEY, "").unwrap();
    for r in recs.iter_mut() {
        if r.entity_type == "servers" {
            let plain = decrypt_entity(r.blob.as_deref().unwrap(), &KEY).unwrap();
            let mut obj: serde_json::Value = serde_json::from_slice(&plain).unwrap();
            obj.as_object_mut().unwrap().remove("terminal_theme");
            r.blob = Some(encrypt_entity(&serde_json::to_vec(&obj).unwrap(), &KEY).unwrap());
            r.updated_at = ha.tick();
        }
    }
    apply_remote_records(&b, &KEY, &recs, &hb).unwrap();
    let (host, theme): (String, Option<String>) = b
        .query_row("SELECT host, terminal_theme FROM servers WHERE name='prod'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(host, "h2", "the older peer's edit must apply");
    assert_eq!(theme.as_deref(), Some("nord"), "an absent key must keep the local scheme");

    // A new device inserting the older peer's record gets no scheme.
    let (c, hc) = device("C");
    apply_remote_records(&c, &KEY, &recs, &hc).unwrap();
    let theme: Option<String> = c.query_row("SELECT terminal_theme FROM servers WHERE name='prod'", [], |r| r.get(0)).unwrap();
    assert_eq!(theme, None);

    // An explicit null, from a peer that knows the column but stores the
    // app's default as NULL (a build from before NULL_IS_UNKNOWN), still
    // clears it.
    let mut recs = collect_local_records(&a, &KEY, "").unwrap();
    for r in recs.iter_mut() {
        if r.entity_type == "servers" {
            let plain = decrypt_entity(r.blob.as_deref().unwrap(), &KEY).unwrap();
            let mut obj: serde_json::Value = serde_json::from_slice(&plain).unwrap();
            obj["terminal_theme"] = serde_json::Value::Null;
            r.blob = Some(encrypt_entity(&serde_json::to_vec(&obj).unwrap(), &KEY).unwrap());
            r.updated_at = ha.tick();
        }
    }
    apply_remote_records(&b, &KEY, &recs, &hb).unwrap();
    let theme: Option<String> = b.query_row("SELECT terminal_theme FROM servers WHERE name='prod'", [], |r| r.get(0)).unwrap();
    assert_eq!(theme, None, "a null from a peer that knows the column clears it");
}

// A device that got a server from an older peer (whose record has no
// scheme and no highlight set) holds both as NULL: not known here. Its
// copy of that record, passed on, must not clear them where they're set:
// the store sync sends every row each time, and a merge can pick that
// copy. The app's default, chosen here, is "" and does travel.
#[test]
fn a_scheme_not_known_here_is_not_passed_on_as_cleared() {
    let (a, ha) = device("A");
    a.execute("INSERT INTO servers(name, host, port) VALUES('prod','h1',22)", []).unwrap();
    let id: i32 = a.query_row("SELECT id FROM servers", [], |r| r.get(0)).unwrap();
    set_server_look(&a, "terminal_theme", id, Some("nord")).unwrap();
    set_server_look(&a, "highlight", id, Some("network")).unwrap();
    let (b, hb) = device("B");
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();

    // An older peer edits the host: its record has neither key.
    a.execute("UPDATE servers SET host='h2'", []).unwrap();
    let mut older = collect_local_records(&a, &KEY, "").unwrap();
    for r in older.iter_mut().filter(|r| r.entity_type == "servers") {
        let plain = decrypt_entity(r.blob.as_deref().unwrap(), &KEY).unwrap();
        let mut obj: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        obj.as_object_mut().unwrap().remove("terminal_theme");
        obj.as_object_mut().unwrap().remove("highlight");
        r.blob = Some(encrypt_entity(&serde_json::to_vec(&obj).unwrap(), &KEY).unwrap());
        r.updated_at = ha.tick();
    }
    // C, new, gets the server from it, and passes its copy on to B,
    // which hasn't seen the older peer's edit yet.
    let (c, hc) = device("C");
    apply_remote_records(&c, &KEY, &older, &hc).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&c, &KEY, "").unwrap(), &hb).unwrap();
    let row: (String, Option<String>, Option<String>) = b
        .query_row("SELECT host, terminal_theme, highlight FROM servers", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!(row, ("h2".into(), Some("nord".into()), Some("network".into())), "the edit applies, the look stays");

    // Saving the default over a NULL on C isn't a change.
    let cid: i32 = c.query_row("SELECT id FROM servers", [], |r| r.get(0)).unwrap();
    assert_eq!(set_server_look(&c, "terminal_theme", cid, None).unwrap(), 0);

    // The app's default chosen on A reaches B.
    set_server_look(&a, "terminal_theme", id, None).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let theme: Option<String> = b.query_row("SELECT terminal_theme FROM servers", [], |r| r.get(0)).unwrap();
    assert_eq!(theme.as_deref(), Some(""), "the app's default is a value that travels");
}

#[test]
fn a_servers_highlight_set_syncs_and_an_older_peer_keeps_it() {
    let (a, ha) = device("A");
    a.execute("INSERT INTO servers(name, host, port, highlight) VALUES('core','h1',22,'network')", []).unwrap();
    let (b, hb) = device("B");
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let set: Option<String> = b.query_row("SELECT highlight FROM servers WHERE name='core'", [], |r| r.get(0)).unwrap();
    assert_eq!(set.as_deref(), Some("network"));

    // An older peer's edit has no highlight key: the choice stays.
    a.execute("UPDATE servers SET host='h2' WHERE name='core'", []).unwrap();
    let mut recs = collect_local_records(&a, &KEY, "").unwrap();
    for r in recs.iter_mut().filter(|r| r.entity_type == "servers") {
        let plain = decrypt_entity(r.blob.as_deref().unwrap(), &KEY).unwrap();
        let mut obj: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        obj.as_object_mut().unwrap().remove("highlight");
        r.blob = Some(encrypt_entity(&serde_json::to_vec(&obj).unwrap(), &KEY).unwrap());
        r.updated_at = ha.tick();
    }
    apply_remote_records(&b, &KEY, &recs, &hb).unwrap();
    let (host, set): (String, Option<String>) = b
        .query_row("SELECT host, highlight FROM servers WHERE name='core'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(host, "h2");
    assert_eq!(set.as_deref(), Some("network"));
}

#[test]
fn highlight_rules_sync_edit_and_delete() {
    let (a, ha) = device("A");
    let rule = |pattern: &str| highlight::HighlightRule {
        id: None,
        pattern: pattern.into(),
        kind: "word".into(),
        match_case: false,
        color: "magenta".into(),
        bold: true,
        underline: false,
        enabled: true,
    };
    let id = highlight::save(&a, &rule("deploy")).unwrap();
    highlight::save(&a, &rule("rollback")).unwrap();
    let (b, hb) = device("B");
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let got = highlight::list(&b).unwrap();
    assert_eq!(got.iter().map(|r| r.pattern.as_str()).collect::<Vec<_>>(), ["deploy", "rollback"]);
    assert!(got[0].bold && got[0].color == "magenta");

    // An edit, then a deletion, on A reach B.
    let mut edited = rule("deployed");
    edited.id = Some(id);
    edited.underline = true;
    highlight::save(&a, &edited).unwrap();
    a.execute("DELETE FROM highlight_rules WHERE pattern='rollback'", []).unwrap();
    let mut recs = collect_local_records(&a, &KEY, "").unwrap();
    for r in recs.iter_mut().filter(|r| r.entity_type == "highlight_rules") {
        r.updated_at = ha.tick();
    }
    apply_remote_records(&b, &KEY, &recs, &hb).unwrap();
    let got = highlight::list(&b).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].pattern.as_str(), got[0].underline), ("deployed", true));
}

// Nor to the move in it: a record held for its emptied password that also
// moves the server into a folder at "Run anything" here lands with the
// AI's level held, as any other sync's move would.
#[test]
fn a_yes_to_a_held_record_still_holds_the_ai_s_level() {
    let (a, _ha) = device("A");
    a.execute_batch(
        "INSERT INTO folders(name) VALUES('Lab');
         INSERT INTO servers(name,host,port,password) VALUES('db-01','h',22,'pw');",
    )
    .unwrap();
    let (b, hb) = member("B", "editor");
    ai::trust::create_schema(&b, "this-install-0000").unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    let lab: i64 = b.query_row("SELECT id FROM folders WHERE name='Lab'", [], |r| r.get(0)).unwrap();
    ai::trust::set_level(&b, "folder", lab, Some(3)).unwrap();
    a.execute("UPDATE servers SET password='', folder_id=(SELECT id FROM folders WHERE name='Lab')", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert_eq!(approvals::held_wipes(&b).len(), 1, "held for the password");
    let (db, uuid): (i64, String) = b.query_row("SELECT id, uuid FROM servers", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    resolve_held(&b, &KEY, &hb, true).unwrap();
    let folder: Option<i64> = b.query_row("SELECT folder_id FROM servers", [], |r| r.get(0)).unwrap();
    assert_eq!(folder, Some(lab), "the move landed");
    assert!(
        approvals::held_for(&b, &uuid).iter().any(|i| i.starts_with(ai::trust::HELD_ITEM)),
        "the AI's level is held"
    );
    let p = ai::trust::policy_for(&b, Some(db));
    assert!(p.force_ask.is_some(), "every command asks until it's allowed");
}

// A yes to a held deletion of a jump host leaves the servers behind it
// refusing to go direct, as a sync's deletion of it does (jump_gone.rs).
#[test]
fn a_yes_to_a_held_deletion_of_a_jump_host_notes_the_servers_behind_it() {
    let (a, _ha) = device("A");
    for i in 0..15 {
        a.execute("INSERT INTO servers(name,host,port) VALUES(?1,'h',22)", [format!("s{i:02}")]).unwrap();
    }
    a.execute("UPDATE servers SET jump_host_id=(SELECT id FROM servers WHERE name='s05') WHERE name='s00'", []).unwrap();
    let (b, hb) = shared_device("B");
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    a.execute("DELETE FROM servers WHERE name >= 's03'", []).unwrap();
    apply_remote_records(&b, &KEY, &collect_local_records(&a, &KEY, "").unwrap(), &hb).unwrap();
    assert_eq!(approvals::held_deletions(&b).len(), 12);
    let s00: i64 = b.query_row("SELECT id FROM servers WHERE name='s00'", [], |r| r.get(0)).unwrap();
    assert!(jump_gone::reason(&b, s00).is_none());
    resolve_held(&b, &KEY, &hb, true).unwrap();
    assert_eq!(count(&b, "servers"), 3);
    assert!(jump_gone::reason(&b, s00).is_some(), "it won't go direct without asking");
}

// `a_referent_deleted_in_the_same_batch_clears_the_link`, and then: a server
// whose jump host a sync deleted isn't connected directly without asking
// (jump_gone.rs), on a device that had the jump host or one that never did.
#[test]
fn a_jump_host_deleted_by_a_sync_leaves_its_servers_refusing_to_go_direct() {
    let (a, _ha) = device("A");
    a.execute("INSERT INTO servers(name,host,port) VALUES('bastion','b',22)", []).unwrap();
    a.execute(
        "INSERT INTO servers(name,host,port,jump_host_id) VALUES('app','a',22,(SELECT id FROM servers WHERE name='bastion'))",
        [],
    )
    .unwrap();
    let seed = collect_local_records(&a, &KEY, "").unwrap();
    let (c, hc) = device("C");
    apply_remote_records(&c, &KEY, &seed, &hc).unwrap();
    a.execute("UPDATE servers SET host='a2' WHERE name='app'", []).unwrap();
    a.execute("DELETE FROM servers WHERE name='bastion'", []).unwrap();
    let batch = collect_local_records(&a, &KEY, "").unwrap();
    assert!(batch.iter().any(|r| r.deleted), "batch must carry the tombstone");
    apply_remote_records(&c, &KEY, &batch, &hc).unwrap();
    let dangling: Option<i64> = c
        .query_row("SELECT jump_host_id FROM servers WHERE name='app'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dangling, None, "the link must be cleared, not left dangling");
    // ...and C won't connect it directly without asking (jump_gone.rs).
    let app_id: i64 = c.query_row("SELECT id FROM servers WHERE name='app'", [], |r| r.get(0)).unwrap();
    let refused = crate::jump_gone::refuse(&c, app_id).unwrap_err();
    assert!(refused.starts_with("[JUMP_GONE] app's jump host (bastion) was deleted"), "{refused}");
    // A device that never had the bastion gets 'app' with a jump host it
    // can't find: refused too, without the name.
    let (d, hd) = device("D");
    let app_uuid: String = a.query_row("SELECT uuid FROM servers WHERE name='app'", [], |r| r.get(0)).unwrap();
    let app_only: Vec<SyncRecord> = seed.iter().filter(|r| r.uuid == app_uuid).cloned().collect();
    assert_eq!(app_only.len(), 1);
    apply_remote_records(&d, &KEY, &app_only, &hd).unwrap();
    let d_app: i64 = d.query_row("SELECT id FROM servers WHERE name='app'", [], |r| r.get(0)).unwrap();
    assert!(crate::jump_gone::refuse(&d, d_app).is_err(), "no jump host it can find: refused");
}

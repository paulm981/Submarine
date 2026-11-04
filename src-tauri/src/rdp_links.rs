//! Remote Desktop links, the fork's own before RDP servers came in from the
//! original project (issue #42): a saved RDP target that opened the system's
//! Remote Desktop app. Each one is now an RDP server set to open in that app
//! (`app: "system"`, the RDP options' default), so this module only moves
//! them over.
//!
//! The `rdp_links` table stays, and still syncs (lib.rs `ENTITIES`): a device
//! on an older version may still have links, or add one. Whatever comes in is
//! moved over here too, after the merge, and its deletion goes back out, so
//! that device gets the RDP server in its place.
//!
//! Every device that moves a link gives its server the same id, derived from
//! the link's (`server_uuid`), so two devices moving the same link make one
//! server, not two. A link whose server is already here, or was deleted here,
//! is only removed: an older device's later edit to the link doesn't change a
//! server someone has since edited (in a shared profile, a new address that
//! way would skip the question a moved server asks: approvals.rs).
//!
//! Links never kept a password, so neither does the server.

use rusqlite::{params, Connection, OptionalExtension};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rdp_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 3389,
    username TEXT NOT NULL DEFAULT '',
    folder_id INTEGER,
    notes TEXT NOT NULL DEFAULT ''
)";

/// The id of the RDP server a link becomes: the same on every device.
fn server_uuid(link_uuid: &str) -> Option<String> {
    crate::derived_entity_uuid("rdp_link_server", &[Some(link_uuid.to_string())])
}

struct Link {
    id: i64,
    uuid: Option<String>,
    name: String,
    host: String,
    port: i64,
    username: String,
    folder_id: Option<i64>,
    notes: String,
}

/// Move every link into an RDP server, in one transaction. Runs outside a
/// merge (the merge flag off), so the new servers and the links' deletions
/// are stamped as this device's edits and go out at the next sync. Returns
/// how many links there were.
pub(crate) fn move_into_servers(conn: &Connection) -> Result<usize, String> {
    let db = |e: rusqlite::Error| format!("[DATABASE] RDP_LINK_MOVE: {e}");
    let has_links: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='rdp_links')", [], |r| r.get(0))
        .map_err(db)?;
    if !has_links {
        return Ok(0);
    }
    let links: Vec<Link> = {
        let mut stmt = conn
            .prepare("SELECT id, uuid, name, host, port, username, folder_id, notes FROM rdp_links ORDER BY id")
            .map_err(db)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Link {
                    id: r.get(0)?,
                    uuid: r.get(1)?,
                    name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    host: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    port: r.get::<_, Option<i64>>(4)?.unwrap_or(3389),
                    username: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    folder_id: r.get(6)?,
                    notes: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                })
            })
            .map_err(db)?;
        rows.collect::<Result<_, _>>().map_err(db)?
    };
    if links.is_empty() {
        return Ok(0);
    }
    let tx = conn.unchecked_transaction().map_err(db)?;
    for link in &links {
        let uuid = link.uuid.as_deref().and_then(server_uuid);
        let known = match &uuid {
            Some(u) => tx
                .query_row(
                    "SELECT 1 FROM servers WHERE uuid = ?1 UNION ALL SELECT 1 FROM sync_tombstones WHERE uuid = ?1 LIMIT 1",
                    [u],
                    |_| Ok(()),
                )
                .optional()
                .map_err(db)?
                .is_some(),
            None => false,
        };
        if !known {
            // Its folder may have gone (a link's folder has no foreign key).
            let folder_id: Option<i64> = match link.folder_id {
                Some(f) => tx.query_row("SELECT id FROM folders WHERE id = ?1", [f], |r| r.get(0)).optional().map_err(db)?,
                None => None,
            };
            let position: i64 = tx
                .query_row("SELECT COALESCE(MAX(position) + 1, 0) FROM servers WHERE folder_id IS ?1", [folder_id], |r| r.get(0))
                .map_err(db)?;
            let port = link.port.clamp(1, 65535);
            tx.execute(
                "INSERT INTO servers (name, host, port, username, password, credential_id, folder_id, proxy_type, proxy_host, proxy_port,
                                      tunnels, auth_type, key_id, autostart, mirrors, color, notes, run_on_connect, position, protocol)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, 'none', '', 1080, '[]', 'custom_pass', NULL, 0, '[]', NULL, ?6, '', ?7, 'rdp')",
                params![link.name.trim(), link.host.trim(), port, link.username.trim(), folder_id, link.notes, position],
            )
            .map_err(db)?;
            // The insert trigger gave it a random id and this device's stamp;
            // the derived id is what makes two devices' moves one server.
            if let Some(u) = &uuid {
                tx.execute("UPDATE servers SET uuid = ?1 WHERE id = ?2", params![u, tx.last_insert_rowid()]).map_err(db)?;
            }
        }
        tx.execute("DELETE FROM rdp_links WHERE id = ?1", [link.id]).map_err(db)?;
    }
    tx.commit().map_err(db)?;
    Ok(links.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync_engine_tests::device;

    const KEY: [u8; 32] = [7u8; 32];

    type Row = (String, String, i64, String, Option<String>, Option<i64>);

    fn servers(c: &Connection) -> Vec<Row> {
        let mut stmt = c.prepare("SELECT name, host, port, username, protocol, folder_id FROM servers ORDER BY name").unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap();
        rows.map(Result::unwrap).collect()
    }

    fn count(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn sync(from: &Connection, to: &Connection, hlc: &crate::hlc::Hlc) {
        crate::apply_remote_records(to, &KEY, &crate::collect_local_records(from, &KEY, "").unwrap(), hlc).unwrap();
    }

    // A link becomes an RDP server with its name, address, user name, notes
    // and folder, and no password; the link is gone, with a deletion to sync.
    #[test]
    fn a_link_becomes_an_rdp_server_in_its_folder() {
        let (a, _ha) = device("A");
        a.execute_batch(
            "INSERT INTO folders(name) VALUES('Office');
             INSERT INTO rdp_links(name, host, port, username, folder_id, notes)
               VALUES('desk', 'desk.example', 3390, 'CORP\\me', (SELECT id FROM folders WHERE name='Office'), 'by the window'),
                     ('lab', 'lab.example', 3389, '', 99, '');",
        )
        .unwrap();
        assert_eq!(move_into_servers(&a).unwrap(), 2);
        let office: i64 = a.query_row("SELECT id FROM folders WHERE name='Office'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            servers(&a),
            vec![
                ("desk".into(), "desk.example".into(), 3390, "CORP\\me".into(), Some("rdp".into()), Some(office)),
                ("lab".into(), "lab.example".into(), 3389, "".into(), Some("rdp".into()), None),
            ]
        );
        assert_eq!(count(&a, "SELECT COUNT(*) FROM servers WHERE password IS NOT NULL OR credential_id IS NOT NULL"), 0);
        let notes: String = a.query_row("SELECT notes FROM servers WHERE name='desk'", [], |r| r.get(0)).unwrap();
        assert_eq!(notes, "by the window");
        assert_eq!(count(&a, "SELECT COUNT(*) FROM rdp_links"), 0);
        assert_eq!(count(&a, "SELECT COUNT(*) FROM sync_tombstones WHERE entity_type='rdp_links'"), 2);
        // Nothing left: nothing to do.
        assert_eq!(move_into_servers(&a).unwrap(), 0);
    }

    // Two devices that each move the same synced link make one server.
    #[test]
    fn two_devices_moving_the_same_link_make_one_server() {
        let (a, ha) = device("A");
        a.execute("INSERT INTO rdp_links(name, host) VALUES('desk', 'desk.example')", []).unwrap();
        let (b, hb) = device("B");
        // B moves it once the link has come in (after the merge); A its own.
        sync(&a, &b, &hb);
        assert_eq!(count(&b, "SELECT COUNT(*) FROM servers"), 1);
        move_into_servers(&a).unwrap();
        let uuid = |c: &Connection| -> String { c.query_row("SELECT uuid FROM servers", [], |r| r.get(0)).unwrap() };
        assert_eq!(uuid(&a), uuid(&b));
        sync(&b, &a, &ha);
        sync(&a, &b, &hb);
        for c in [&a, &b] {
            assert_eq!(count(c, "SELECT COUNT(*) FROM servers"), 1);
            assert_eq!(count(c, "SELECT COUNT(*) FROM rdp_links"), 0);
        }
    }

    // A device on an older version gets the server and the link's deletion.
    // An older device's edit to a link already moved doesn't change the
    // server, nor bring the link back.
    #[test]
    fn an_older_devices_link_edit_leaves_the_server_as_it_is() {
        let (old, ho) = device("O");
        old.execute("INSERT INTO rdp_links(name, host) VALUES('desk', 'desk.example')", []).unwrap();
        let (new, hn) = device("N");
        sync(&old, &new, &hn);
        // The older device takes what comes back by the merge alone, moving
        // nothing, as an older version does.
        let back = crate::collect_local_records(&new, &KEY, "").unwrap();
        crate::with_merge_flag(&old, |c| crate::apply_remote_inner(c, &KEY, &back, &ho, usize::MAX)).unwrap();
        assert_eq!(count(&old, "SELECT COUNT(*) FROM rdp_links"), 0, "the link's deletion reached it");
        assert_eq!(count(&old, "SELECT COUNT(*) FROM servers WHERE protocol='rdp'"), 1, "and the server");

        let (old2, _h2) = device("P");
        old2.execute("INSERT INTO rdp_links(name, host) VALUES('desk', 'desk.example')", []).unwrap();
        let (new2, hn2) = device("M");
        sync(&old2, &new2, &hn2);
        // Its stamp has to sort after the deletion's to come back at all.
        std::thread::sleep(std::time::Duration::from_millis(5));
        old2.execute("UPDATE rdp_links SET host='elsewhere.example'", []).unwrap();
        sync(&old2, &new2, &hn2);
        let host: String = new2.query_row("SELECT host FROM servers", [], |r| r.get(0)).unwrap();
        assert_eq!(host, "desk.example", "the server isn't moved by a link");
        assert_eq!(count(&new2, "SELECT COUNT(*) FROM servers"), 1);
        assert_eq!(count(&new2, "SELECT COUNT(*) FROM rdp_links"), 0);
    }

    // A server deleted here isn't brought back by its link.
    #[test]
    fn a_deleted_server_stays_deleted() {
        let (old, _ho) = device("O");
        old.execute("INSERT INTO rdp_links(name, host) VALUES('desk', 'desk.example')", []).unwrap();
        let (new, hn) = device("N");
        let recs = crate::collect_local_records(&old, &KEY, "").unwrap();
        crate::apply_remote_records(&new, &KEY, &recs, &hn).unwrap();
        new.execute("DELETE FROM servers", []).unwrap();
        // The same link again, as an older device that synced late sends it.
        new.execute("DELETE FROM sync_tombstones WHERE entity_type='rdp_links'", []).unwrap();
        crate::apply_remote_records(&new, &KEY, &recs, &hn).unwrap();
        assert_eq!(count(&new, "SELECT COUNT(*) FROM servers"), 0);
        assert_eq!(count(&new, "SELECT COUNT(*) FROM rdp_links"), 0);
    }
}

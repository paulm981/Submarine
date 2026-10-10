//! Turning the hosts picked in the import dialog into server rows, in one go.
//!
//! An import from MobaXterm carries more than host, port and user: the
//! session's folder, the saved login it signs in with (`[name]` as the user),
//! its jump host and the key that jump host takes, and the command it runs
//! once logged in. Everything is written in one transaction with one vault
//! save, so a few hundred sessions import in a moment and a failure leaves
//! nothing half done:
//!
//! - folders are created as nested folders, reusing ones that already exist;
//! - each saved login becomes one Login (Settings › Logins), reused by name,
//!   with its user name and password left for the user to fill in once;
//! - a single-hop jump host is matched to a server already saved with the
//!   same host, port and user, or saved as a new one with its key, and every
//!   host behind it is linked to it. Submarine jumps through one host only,
//!   so a chain of several is reported and left for the user;
//! - a server already saved with the same name, host, port and folder is
//!   left alone, so importing the same file twice adds nothing.

use crate::ImportedHost;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// One picked host, with the keys the frontend already put in the vault
/// (`import_ssh_key_file`) for its own key and its jump host's.
#[derive(serde::Deserialize)]
pub(crate) struct ImportRow {
    pub host: ImportedHost,
    #[serde(default)]
    pub key_id: Option<i64>,
    #[serde(default)]
    pub jump_key_id: Option<i64>,
}

#[derive(serde::Serialize, Default, Debug)]
pub(crate) struct ImportReport {
    /// Server rows added.
    pub added: usize,
    /// Rows already saved (same name, host, port and folder), left alone.
    pub already_there: usize,
    pub folders_created: usize,
    /// Logins created for saved-login names; the user fills them in.
    pub logins_created: Vec<String>,
    /// Jump hosts saved as servers of their own.
    pub jump_hosts_created: Vec<String>,
    /// Hosts linked to a jump host.
    pub jump_linked: usize,
    /// Hosts whose jump chain has more than one hop: imported without one.
    pub multi_hop: Vec<String>,
}

/// `[user@]host[:port]`, with `[v6]:port` for IPv6 literals.
fn parse_hop(hop: &str) -> Option<(String, String, u16)> {
    let hop = hop.trim();
    let (user, rest) = match hop.rsplit_once('@') {
        Some((u, r)) => (u.trim().to_string(), r.trim()),
        None => (String::new(), hop),
    };
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        let port = tail.strip_prefix(':').map(|p| p.parse::<u16>().ok()).unwrap_or(Some(22))?;
        (h.to_string(), port)
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h.to_string(), p.parse::<u16>().ok()?),
            _ => (rest.to_string(), 22),
        }
    };
    (!host.is_empty()).then_some((user, host, port))
}

fn short_host(host: &str) -> &str {
    if host.parse::<std::net::IpAddr>().is_ok() {
        host
    } else {
        host.split('.').next().unwrap_or(host)
    }
}

/// Folder path → id, creating what's missing. Matching is case-insensitive,
/// as MobaXterm's own folder names are.
struct Folders {
    by_parent_name: HashMap<(Option<i64>, String), i64>,
    created: usize,
}

impl Folders {
    fn load(conn: &Connection) -> rusqlite::Result<Self> {
        let mut stmt = conn.prepare("SELECT id, name, parent_id FROM folders")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?))
        })?;
        let mut by_parent_name = HashMap::new();
        for row in rows {
            let (id, name, parent) = row?;
            by_parent_name.entry((parent, name.unwrap_or_default().trim().to_lowercase())).or_insert(id);
        }
        Ok(Folders { by_parent_name, created: 0 })
    }

    fn resolve(&mut self, conn: &Connection, path: &[String]) -> rusqlite::Result<Option<i64>> {
        let mut parent: Option<i64> = None;
        for segment in path.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let key = (parent, segment.to_lowercase());
            let id = match self.by_parent_name.get(&key) {
                Some(id) => *id,
                None => {
                    conn.execute("INSERT INTO folders (name, parent_id) VALUES (?1, ?2)", params![segment, parent])?;
                    let id = conn.last_insert_rowid();
                    self.by_parent_name.insert(key, id);
                    self.created += 1;
                    id
                }
            };
            parent = Some(id);
        }
        Ok(parent)
    }
}

fn login_id(conn: &Connection, logins: &mut HashMap<String, i64>, name: &str, report: &mut ImportReport) -> rusqlite::Result<i64> {
    let key = name.trim().to_lowercase();
    if let Some(id) = logins.get(&key) {
        return Ok(*id);
    }
    conn.execute(
        "INSERT INTO credentials (name, auth_type, username, password, key_id) VALUES (?1, 'password', '', NULL, NULL)",
        params![name.trim()],
    )?;
    let id = conn.last_insert_rowid();
    logins.insert(key, id);
    report.logins_created.push(name.trim().to_string());
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn insert_server(
    conn: &Connection,
    name: &str,
    host: &str,
    port: u16,
    username: &str,
    credential_id: Option<i64>,
    key_id: Option<i64>,
    folder_id: Option<i64>,
    run_on_connect: &str,
) -> rusqlite::Result<i64> {
    // Same rule as add_server (normalize_server_identity): a server signs in
    // with a Login, or with its own user and key, or its own user and
    // password, never a mix.
    let (auth_type, username, credential_id, key_id) = match (key_id, credential_id) {
        (Some(k), _) => ("custom_key", Some(username), None, Some(k)),
        (None, Some(c)) => ("vault", None, Some(c), None),
        (None, None) => ("custom_pass", Some(username), None, None),
    };
    let username = username.map(str::trim).filter(|u| !u.is_empty());
    conn.execute(
        "INSERT INTO servers (name, host, port, username, password, credential_id, folder_id, proxy_type, proxy_host, proxy_port, tunnels, auth_type, key_id, autostart, mirrors, color, notes, run_on_connect)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 'none', '', 1080, '[]', ?7, ?8, 0, '[]', NULL, '', ?9)",
        params![name, host, port, username, credential_id, folder_id, auth_type, key_id, run_on_connect],
    )?;
    let id = conn.last_insert_rowid();
    // To the bottom of its folder, as add_server does.
    conn.execute(
        "UPDATE servers SET position = COALESCE((SELECT MAX(position) FROM servers WHERE id != ?1 AND folder_id IS ?2), -1) + 1 WHERE id = ?1",
        params![id, folder_id],
    )?;
    Ok(id)
}

/// Write `rows` into the open vault. The caller runs it in a transaction and
/// saves the vault afterwards.
pub(crate) fn import_rows(conn: &Connection, rows: &[ImportRow]) -> Result<ImportReport, String> {
    let db = |e: rusqlite::Error| format!("[DATABASE] IMPORT_FAILED: {}", e);
    let mut report = ImportReport::default();
    let mut folders = Folders::load(conn).map_err(db)?;
    let mut logins: HashMap<String, i64> = {
        let mut stmt = conn.prepare("SELECT id, name FROM credentials").map_err(db)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))
            .map_err(db)?;
        let mut m = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(db)?;
            m.entry(name.unwrap_or_default().trim().to_lowercase()).or_insert(id);
        }
        m
    };

    // Rows added here, by import key, for jump hosts named by alias
    // (an ssh_config `ProxyJump bastion` naming another Host block).
    let mut by_alias: HashMap<String, i64> = HashMap::new();
    let mut pending_jumps: Vec<(i64, &ImportRow)> = Vec::new();

    for row in rows {
        let h = &row.host;
        let name = h.host_alias.trim();
        let host = if h.hostname.trim().is_empty() { name } else { h.hostname.trim() };
        let port = if h.port == 0 { 22 } else { h.port };
        let folder_id = folders.resolve(conn, &h.folder).map_err(db)?;

        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM servers WHERE name = ?1 AND host = ?2 AND port = ?3 AND folder_id IS ?4 LIMIT 1",
                params![name, host, port, folder_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if let Some(id) = existing {
            report.already_there += 1;
            by_alias.entry(name.to_lowercase()).or_insert(id);
            continue;
        }

        let credential_id = match h.credential.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => Some(login_id(conn, &mut logins, c, &mut report).map_err(db)?),
            None => None,
        };
        let run = h.run_on_connect.as_deref().map(str::trim).unwrap_or("");
        let id = insert_server(conn, name, host, port, &h.user, credential_id, row.key_id, folder_id, run).map_err(db)?;
        report.added += 1;
        by_alias.entry(name.to_lowercase()).or_insert(id);
        if h.proxy_jump.as_deref().is_some_and(|j| !j.trim().is_empty()) {
            pending_jumps.push((id, row));
        }
    }

    // Jump hosts, once every picked host is in, so one can name another.
    let mut created_jumps: HashMap<(String, u16, String), i64> = HashMap::new();
    for (id, row) in pending_jumps {
        let spec = row.host.proxy_jump.as_deref().unwrap_or("").trim();
        let hops: Vec<&str> = spec.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if hops.len() != 1 {
            report.multi_hop.push(row.host.host_alias.trim().to_string());
            continue;
        }
        let Some((user, host, port)) = parse_hop(hops[0]) else {
            report.multi_hop.push(row.host.host_alias.trim().to_string());
            continue;
        };

        let alias_hit = (user.is_empty() && !hops[0].contains(':'))
            .then(|| by_alias.get(&host.to_lowercase()).copied())
            .flatten()
            .filter(|j| *j != id);
        let key = (host.to_lowercase(), port, user.clone());
        let jump_id = if let Some(j) = alias_hit {
            j
        } else if let Some(j) = created_jumps.get(&key) {
            *j
        } else {
            // A server already saved for this host, port and user (any user
            // when the hop names none), not signing in with a Login unless no
            // user is asked for.
            let found: Option<i64> = conn
                .query_row(
                    "SELECT id FROM servers
                     WHERE lower(host) = lower(?1) AND port = ?2 AND id != ?3
                       AND (?4 = '' OR username = ?4)
                     ORDER BY jump_host_id IS NOT NULL, id LIMIT 1",
                    params![host, port, id, user],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db)?;
            let j = match found {
                Some(j) => j,
                None => {
                    let name = format!("{} (jump host)", short_host(&host));
                    let j = insert_server(conn, &name, &host, port, &user, None, row.jump_key_id, None, "").map_err(db)?;
                    report.jump_hosts_created.push(name);
                    j
                }
            };
            created_jumps.insert(key, j);
            j
        };
        conn.execute("UPDATE servers SET jump_host_id = ?1 WHERE id = ?2", params![jump_id, id]).map_err(db)?;
        report.jump_linked += 1;
    }

    report.folders_created = folders.created;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE folders(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, parent_id INTEGER, color TEXT);
             CREATE TABLE ssh_keys(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, public_key TEXT, private_key TEXT, passphrase TEXT);
             CREATE TABLE credentials(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, auth_type TEXT, username TEXT, password TEXT, key_id INTEGER);
             CREATE TABLE servers(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, host TEXT, port INTEGER, username TEXT, password TEXT, credential_id INTEGER, folder_id INTEGER, proxy_type TEXT, proxy_host TEXT, proxy_port INTEGER, tunnels TEXT, auth_type TEXT, key_id INTEGER, autostart INTEGER, mirrors TEXT, color TEXT, notes TEXT, run_on_connect TEXT, jump_host_id INTEGER, position INTEGER NOT NULL DEFAULT 0);
             INSERT INTO ssh_keys(name, private_key) VALUES ('id_jump', 'k1'), ('own', 'k2');",
        )
        .unwrap();
        conn
    }

    fn moba(name: &str, folder: &[&str], host: &str, login: Option<&str>, jump: Option<&str>) -> ImportRow {
        ImportRow {
            host: ImportedHost {
                host_alias: name.into(),
                hostname: host.into(),
                port: 22,
                folder: folder.iter().map(|s| s.to_string()).collect(),
                credential: login.map(Into::into),
                run_on_connect: Some("sudo -i".into()),
                proxy_jump: jump.map(Into::into),
                ..Default::default()
            },
            key_id: None,
            jump_key_id: jump.map(|_| 1),
        }
    }

    #[test]
    fn folders_logins_jump_host_and_command_come_across() {
        let conn = vault();
        let rows = vec![
            moba("web1", &["Prod", "East"], "web1.example.net", Some("OPS"), Some("jump@bastion.example.net")),
            moba("web2", &["Prod", "East"], "web2.example.net", Some("OPS"), Some("jump@bastion.example.net")),
            moba("db1", &["Prod", "West"], "db1.example.net", Some("ops"), Some("jump@bastion.example.net:22")),
            moba("vm", &[], "10.0.0.5", None, None),
        ];
        let r = import_rows(&conn, &rows).unwrap();
        assert_eq!(r.added, 4);
        assert_eq!(r.folders_created, 3, "Prod, its East and West");
        assert_eq!(r.logins_created, vec!["OPS"], "one Login, matched case-insensitively");
        assert_eq!(r.jump_hosts_created, vec!["bastion (jump host)"]);
        assert_eq!(r.jump_linked, 3);

        let (jid, juser, jauth, jkey): (i64, String, String, i64) = conn
            .query_row("SELECT id, username, auth_type, key_id FROM servers WHERE name = 'bastion (jump host)'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((juser.as_str(), jauth.as_str(), jkey), ("jump", "custom_key", 1));

        let (auth, cred, user, run, jump, folder): (String, Option<i64>, Option<String>, String, Option<i64>, String) = conn
            .query_row(
                "SELECT s.auth_type, s.credential_id, s.username, s.run_on_connect, s.jump_host_id, f.name
                 FROM servers s JOIN folders f ON f.id = s.folder_id WHERE s.name = 'db1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .unwrap();
        assert_eq!((auth.as_str(), cred.is_some(), user), ("vault", true, None));
        assert_eq!(run, "sudo -i");
        assert_eq!(jump, Some(jid));
        assert_eq!(folder, "West");
        let parent: String = conn
            .query_row("SELECT p.name FROM folders f JOIN folders p ON p.id = f.parent_id WHERE f.name = 'West'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(parent, "Prod");
    }

    #[test]
    fn importing_twice_adds_nothing() {
        let conn = vault();
        let rows = vec![
            moba("web1", &["A"], "web1.example", Some("OPS"), Some("ops@bastion.example")),
            moba("web2", &["A", "B"], "web2.example", Some("OPS"), Some("ops@bastion.example")),
        ];
        import_rows(&conn, &rows).unwrap();
        let again = import_rows(&conn, &rows).unwrap();
        assert_eq!((again.added, again.already_there, again.folders_created), (0, 2, 0));
        assert!(again.logins_created.is_empty() && again.jump_hosts_created.is_empty());
        let servers: i64 = conn.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0)).unwrap();
        assert_eq!(servers, 3);
    }

    #[test]
    fn an_already_saved_jump_host_is_reused() {
        let conn = vault();
        conn.execute(
            "INSERT INTO servers (name, host, port, username, auth_type) VALUES ('my bastion', 'Bastion.example', 22, 'ops', 'custom_key')",
            [],
        )
        .unwrap();
        let r = import_rows(&conn, &[moba("web1", &[], "web1.example", None, Some("ops@bastion.example"))]).unwrap();
        assert!(r.jump_hosts_created.is_empty());
        let jump: String = conn
            .query_row("SELECT j.name FROM servers s JOIN servers j ON j.id = s.jump_host_id WHERE s.name = 'web1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jump, "my bastion");
    }

    #[test]
    fn a_jump_named_by_alias_links_to_that_host() {
        let conn = vault();
        let mut bastion = moba("bastion", &[], "10.0.0.1", None, None);
        bastion.host.run_on_connect = None;
        let rows = vec![moba("app", &[], "10.0.0.2", None, Some("bastion")), bastion];
        let r = import_rows(&conn, &rows).unwrap();
        assert_eq!((r.added, r.jump_linked), (2, 1));
        assert!(r.jump_hosts_created.is_empty());
        let jump: String = conn
            .query_row("SELECT j.host FROM servers s JOIN servers j ON j.id = s.jump_host_id WHERE s.name = 'app'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jump, "10.0.0.1");
    }

    #[test]
    fn a_chain_of_jumps_is_reported_not_guessed() {
        let conn = vault();
        let r = import_rows(&conn, &[moba("deep", &[], "deep.example", None, Some("a@one.example,two.example:2222"))]).unwrap();
        assert_eq!(r.multi_hop, vec!["deep"]);
        assert_eq!(r.jump_linked, 0);
        let jump: Option<i64> = conn.query_row("SELECT jump_host_id FROM servers WHERE name = 'deep'", [], |r| r.get(0)).unwrap();
        assert_eq!(jump, None);
    }

    #[test]
    fn an_own_key_wins_over_a_login() {
        let conn = vault();
        let mut row = moba("git", &[], "git.example", Some("git-login"), None);
        row.host.user = "git".into();
        row.key_id = Some(2);
        import_rows(&conn, &[row]).unwrap();
        let (auth, user, key, cred): (String, Option<String>, Option<i64>, Option<i64>) = conn
            .query_row("SELECT auth_type, username, key_id, credential_id FROM servers WHERE name = 'git'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((auth.as_str(), user.as_deref(), key, cred), ("custom_key", Some("git"), Some(2), None));
    }

    #[test]
    fn hops_parse() {
        assert_eq!(parse_hop("ops@host:2222"), Some(("ops".into(), "host".into(), 2222)));
        assert_eq!(parse_hop("host"), Some(("".into(), "host".into(), 22)));
        assert_eq!(parse_hop("u@[fe80::1]:22"), Some(("u".into(), "fe80::1".into(), 22)));
        assert_eq!(parse_hop("fe80::1"), Some(("".into(), "fe80::1".into(), 22)));
        assert_eq!(parse_hop("host:notaport"), None);
        assert_eq!(parse_hop(""), None);
    }
}

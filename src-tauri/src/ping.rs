//! Ping a server from the session list (#33): is it up, and does its port
//! (SSH's, or the one its protocol uses) take a connection yet, while it
//! reboots, say.
//!
//! One check per call, no task left behind: the panel asks every couple of
//! seconds while it's open and stops asking when it closes. A check is a plain
//! TCP connect, so it needs no privileges (ICMP would) and works where ping is
//! blocked. Nothing is sent and nobody logs in: the connection is closed as
//! soon as it's made.
//!
//! A server behind a saved jump host is checked from the jump host, over a
//! connection Submarine already has to it (the route of a tab on another
//! server behind it, or a tab open on the jump host itself), with a
//! `direct-tcpip` channel that's closed as soon as it opens, even when the
//! jump host answers after the page has stopped waiting. One such check per
//! server is in flight at a time, and none holds a lock other tabs need. Without one, the check says so instead of logging in
//! anywhere. A server reached through a proxy or a ProxyCommand isn't checked
//! from here either: a connect from here wouldn't go the way its logins do.

use crate::diagnose::Reach;
use crate::ssh_manager::{ClientHandler, SshState};
use crate::{ConfigTarget, ConnectRoute, DbState};
use russh::client::Handle;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long one check waits for an answer.
const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// What one check found.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PingReply {
    /// The port took the connection, in `ms`. `through`: the jump host it
    /// was checked from.
    Answers { port: u16, ms: u64, through: Option<String> },
    /// The machine answered, but its port turned the connection down: it's
    /// up, and what listens there isn't yet.
    Refused { port: u16 },
    /// Nothing answered in time, or the connect failed (`why`, when it says
    /// more than "no answer").
    NoAnswer { port: u16, why: Option<String>, through: Option<String> },
    /// Not checked from here, and why, in a few words.
    NotChecked { why: String },
}

/// What a plain TCP connect from here came to.
fn from_reach(port: u16, reach: Result<Reach, String>) -> PingReply {
    match reach {
        Ok(Reach::Answers(ms)) => PingReply::Answers { port, ms: ms as u64, through: None },
        Ok(Reach::Refused) => PingReply::Refused { port },
        Ok(Reach::TimedOut) => PingReply::NoAnswer { port, why: None, through: None },
        Ok(Reach::Unreachable) => PingReply::NoAnswer { port, why: Some("network unreachable".into()), through: None },
        Ok(Reach::Failed(e)) => PingReply::NoAnswer { port, why: Some(e), through: None },
        Err(why) => PingReply::NoAnswer { port, why: Some(format!("the name doesn't resolve: {why}")), through: None },
    }
}

/// The port a server's protocol uses when none is saved.
fn default_port(protocol: Option<&str>) -> u16 {
    match protocol {
        Some("rdp") => crate::rdp::DEFAULT_PORT,
        Some("vnc") => crate::vnc_client::DEFAULT_PORT,
        Some("telnet") => crate::telnet::DEFAULT_PORT,
        _ => 22,
    }
}

struct Server {
    name: Option<String>,
    host: String,
    port: i64,
    user: Option<String>,
    protocol: Option<String>,
    proxy_type: String,
    proxy_host: Option<String>,
    proxy_port: Option<i32>,
    jump_id: Option<i32>,
    jump_name: Option<String>,
    /// Why it mustn't be connected now: its jump host was deleted
    /// (jump_gone.rs).
    refused: Option<String>,
}

fn read_server(db: &DbState, server_id: i32) -> Result<Server, String> {
    let guard = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
    let conn = guard.as_ref().ok_or("Database is not open")?;
    let mut s = conn
        .query_row(
            "SELECT s.name, s.host, s.port,
                    CASE WHEN COALESCE(s.auth_type, 'vault') = 'vault' THEN c.username ELSE s.username END,
                    s.protocol, s.proxy_type, s.proxy_host, s.proxy_port, s.jump_host_id, j.name
             FROM servers s LEFT JOIN credentials c ON c.id = s.credential_id
             LEFT JOIN servers j ON j.id = s.jump_host_id
             WHERE s.id = ?1",
            [server_id],
            |r| {
                Ok(Server {
                    name: r.get(0)?,
                    host: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    port: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    user: r.get(3)?,
                    protocol: r.get(4)?,
                    proxy_type: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "none".into()),
                    proxy_host: r.get(6)?,
                    proxy_port: r.get(7)?,
                    jump_id: r.get(8)?,
                    jump_name: r.get(9)?,
                    refused: None,
                })
            },
        )
        .map_err(|_| "Server not found".to_string())?;
    s.refused = crate::jump_gone::reason(conn, i64::from(server_id));
    Ok(s)
}

/// After the page has been answered, how long a channel open through a jump
/// host is still waited for, so one that opens late is closed rather than
/// left open on the jump host. One that takes longer is let go.
const LATE_OPEN_LIMIT: Duration = Duration::from_secs(30);
/// A tab's own connection to the jump host is shared with the tab, so a
/// check through it isn't waited for past CHECK_TIMEOUT. After one that ran
/// out, that way isn't tried again for this server for a while: an open the
/// jump host answers late can't be closed from here.
const TAB_COOLDOWN: Duration = Duration::from_secs(30);

/// Servers with a check through a jump host still waiting for its channel:
/// one at a time each, so they can't pile up while a server reboots.
fn pending() -> &'static std::sync::Mutex<HashSet<i32>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<HashSet<i32>>> = std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}

fn tab_cooldowns() -> &'static std::sync::Mutex<HashMap<i32, Instant>> {
    static COOLDOWNS: std::sync::OnceLock<std::sync::Mutex<HashMap<i32, Instant>>> = std::sync::OnceLock::new();
    COOLDOWNS.get_or_init(Default::default)
}

/// What a check through a jump host came to.
#[derive(Debug, PartialEq)]
enum Through {
    /// The channel opened, in ms.
    Answers(u64),
    /// The jump host couldn't open it (its words, when it said any).
    NoAnswer(Option<String>),
    /// Nothing yet: it's still being waited for (or an earlier one is).
    NotYet,
}

/// A `direct-tcpip` channel through `via` to `host:port`, closed as soon as
/// it opens. The page is answered within `limit`; the open itself goes on in
/// its own task (holding its own reference to the connection, no lock) for
/// up to LATE_OPEN_LIMIT, so a late one is closed too. One at a time per
/// server: while one is waited for, another check answers NotYet.
async fn open_shared<H: russh::client::Handler + 'static>(
    server_id: i32,
    via: Arc<Handle<H>>,
    host: &str,
    port: u16,
    limit: Duration,
) -> Through {
    if !pending().lock().map(|mut p| p.insert(server_id)).unwrap_or(false) {
        return Through::NotYet;
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let host = host.to_string();
    tokio::spawn(async move {
        let started = Instant::now();
        let open = via.channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0);
        let result = match tokio::time::timeout(LATE_OPEN_LIMIT, open).await {
            Ok(Ok(channel)) => {
                let ms = started.elapsed().as_millis() as u64;
                let _ = channel.close().await;
                Through::Answers(ms)
            }
            Ok(Err(e)) => Through::NoAnswer(Some(e.to_string())),
            Err(_) => Through::NoAnswer(None),
        };
        drop(via);
        if let Ok(mut p) = pending().lock() {
            p.remove(&server_id);
        }
        let _ = tx.send(result);
    });
    match tokio::time::timeout(limit, rx).await {
        Ok(Ok(result)) => result,
        _ => Through::NotYet,
    }
}

/// Through a tab's own connection to the jump host, which the tab uses too:
/// only when it's free right now, and for no longer than `limit`.
async fn open_on_tab(
    server_id: i32,
    tab: &tokio::sync::Mutex<Handle<ClientHandler>>,
    host: &str,
    port: u16,
    limit: Duration,
) -> Option<Through> {
    if tab_cooldowns().lock().ok()?.get(&server_id).is_some_and(|until| Instant::now() < *until) {
        return Some(Through::NotYet);
    }
    let h = tab.try_lock().ok()?;
    if h.is_closed() {
        return None;
    }
    let started = Instant::now();
    let open = h.channel_open_direct_tcpip(host.to_string(), u32::from(port), "127.0.0.1", 0);
    Some(match tokio::time::timeout(limit, open).await {
        Ok(Ok(channel)) => {
            let ms = started.elapsed().as_millis() as u64;
            let _ = channel.close().await;
            Through::Answers(ms)
        }
        Ok(Err(e)) => Through::NoAnswer(Some(e.to_string())),
        Err(_) => {
            if let Ok(mut c) = tab_cooldowns().lock() {
                c.insert(server_id, Instant::now() + TAB_COOLDOWN);
            }
            Through::NotYet
        }
    })
}

/// Check `host:port` from jump host `jump_id`, over a connection Submarine
/// already has to it: None when there's none. A route a tab behind it opened
/// is used first (only one that went through this jump host), then a tab on
/// the jump host itself. No lock is held while the channel opens.
async fn check_through_jump(ssh: &SshState, server_id: i32, jump_id: i32, host: &str, port: u16) -> Option<Through> {
    let route = {
        let routes = ssh.jump_connections.lock().await;
        routes
            .values()
            .filter(|r| r.jump_id == Some(jump_id))
            .filter_map(|r| r.handles.last())
            .find(|h| !h.is_closed())
            .cloned()
    };
    if let Some(via) = route {
        return Some(open_shared(server_id, via, host, port, CHECK_TIMEOUT).await);
    }
    let sessions: Vec<String> =
        ssh.session_servers.lock().await.iter().filter(|(_, id)| **id == jump_id).map(|(s, _)| s.clone()).collect();
    let tabs: Vec<_> = {
        let conns = ssh.connections.lock().await;
        sessions.iter().filter_map(|s| conns.get(s).cloned()).collect()
    };
    for tab in tabs {
        if let Some(through) = open_on_tab(server_id, &tab, host, port, CHECK_TIMEOUT).await {
            return Some(through);
        }
    }
    None
}

/// Check `server_id` once: does its port take a connection, and how fast.
#[tauri::command]
pub async fn ping_server(
    ssh: tauri::State<'_, SshState>,
    db: tauri::State<'_, DbState>,
    server_id: i32,
) -> Result<PingReply, String> {
    let s = read_server(&db, server_id)?;
    // Its jump host was deleted: the same words a terminal gets, and no
    // connection anywhere.
    if let Some(why) = s.refused {
        return Ok(PingReply::NotChecked { why });
    }
    let host = s.host.trim().to_string();
    if host.is_empty() {
        return Ok(PingReply::NotChecked { why: "It has no address saved.".into() });
    }
    let port = u16::try_from(s.port).ok().filter(|p| *p > 0).unwrap_or_else(|| default_port(s.protocol.as_deref()));
    let user = s.user.as_deref().unwrap_or("");
    // Where its logins go: ~/.ssh/config applies to SSH servers only.
    let mut target = if matches!(s.protocol.as_deref(), None | Some("ssh")) {
        ConfigTarget::resolve(&host, port, user, s.name.clone()).await
    } else {
        ConfigTarget::apply(&host, port, user, None)
    };
    let route = crate::connect_route(s.jump_id, &s.proxy_type, s.proxy_host, s.proxy_port, &mut target);
    let port = target.port;
    Ok(match route {
        ConnectRoute::Direct => from_reach(port, crate::diagnose::reach(&target.host, port, CHECK_TIMEOUT).await),
        ConnectRoute::Jump(jump_id) => {
            let through = s.jump_name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "its jump host".into());
            match check_through_jump(&ssh, server_id, jump_id, &target.host, port).await {
                Some(Through::Answers(ms)) => PingReply::Answers { port, ms, through: Some(through) },
                Some(Through::NoAnswer(why)) => PingReply::NoAnswer { port, why, through: Some(through) },
                Some(Through::NotYet) => PingReply::NoAnswer { port, why: Some("no answer yet".into()), through: Some(through) },
                None => PingReply::NotChecked {
                    why: format!("Behind a jump host ({through}): open a terminal to it, or to a server behind it, to check."),
                },
            }
        }
        ConnectRoute::ConfigJump(_) => {
            PingReply::NotChecked { why: "Behind a jump host (ProxyJump in ~/.ssh/config): open a terminal to check.".into() }
        }
        ConnectRoute::Proxy { .. } | ConnectRoute::Command(_) => {
            PingReply::NotChecked { why: "Reached through a proxy: open a terminal to check.".into() }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_whose_jump_host_was_deleted_isnt_pinged() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE servers(id INTEGER PRIMARY KEY, name TEXT, host TEXT, port INTEGER, username TEXT, credential_id INTEGER,
                auth_type TEXT, protocol TEXT, proxy_type TEXT, proxy_host TEXT, proxy_port INTEGER, jump_host_id INTEGER, uuid TEXT);
             CREATE TABLE credentials(id INTEGER PRIMARY KEY, username TEXT);
             CREATE TABLE sync_flags(key TEXT PRIMARY KEY, val INTEGER NOT NULL);
             INSERT INTO servers(id, name, host, port, jump_host_id) VALUES (1, 'web-gw', 'g.example', 22, NULL), (2, 'db-01', 'd.example', 22, 1);",
        )
        .unwrap();
        crate::tree_moves::unlink_jump_hosts(&conn, &[1]).unwrap();
        conn.execute("DELETE FROM servers WHERE id = 1", []).unwrap();
        let db = crate::DbState {
            conn: std::sync::Arc::new(std::sync::Mutex::new(Some(conn))),
            master_key: Default::default(),
            salt: Default::default(),
            db_path: Default::default(),
            active_profile: Default::default(),
            hlc: Default::default(),
            generation: Default::default(),
            sync_cursor: Default::default(),
            profile_lock: Default::default(),
        };
        let s = read_server(&db, 2).unwrap();
        assert_eq!(
            s.refused.as_deref(),
            Some("db-01's jump host (web-gw) was deleted, so Submarine won't connect it directly without asking.")
        );
    }

    #[test]
    fn replies_say_what_the_connect_came_to() {
        assert_eq!(from_reach(22, Ok(Reach::Answers(23))), PingReply::Answers { port: 22, ms: 23, through: None });
        assert_eq!(from_reach(22, Ok(Reach::Refused)), PingReply::Refused { port: 22 });
        assert_eq!(from_reach(22, Ok(Reach::TimedOut)), PingReply::NoAnswer { port: 22, why: None, through: None });
        assert!(matches!(from_reach(22, Err("no such host".into())), PingReply::NoAnswer { why: Some(w), .. } if w.contains("no such host")));
    }

    #[test]
    fn replies_reach_the_page_tagged() {
        let json = serde_json::to_value(PingReply::Answers { port: 22, ms: 23, through: Some("bastion".into()) }).unwrap();
        assert_eq!(json, serde_json::json!({ "kind": "answers", "port": 22, "ms": 23, "through": "bastion" }));
        let json = serde_json::to_value(PingReply::NotChecked { why: "x".into() }).unwrap();
        assert_eq!(json["kind"], "not_checked");
    }

    #[test]
    fn a_server_without_a_port_uses_its_protocols() {
        assert_eq!(default_port(None), 22);
        assert_eq!(default_port(Some("ssh")), 22);
        assert_eq!(default_port(Some("rdp")), 3389);
        assert_eq!(default_port(Some("vnc")), 5900);
        assert_eq!(default_port(Some("telnet")), 23);
    }

    #[tokio::test]
    async fn through_a_jump_host_the_channel_is_the_answer() {
        use crate::ssh_test_server::{connect, TestServer};
        let jump = Arc::new(connect(TestServer::default(), |_| {}, russh::client::Config::default()).await.unwrap());
        assert!(matches!(open_shared(-1, Arc::clone(&jump), "target", 22, CHECK_TIMEOUT).await, Through::Answers(_)));
        assert!(!pending().lock().unwrap().contains(&-1), "nothing left waiting");
    }

    #[tokio::test]
    async fn a_late_channel_is_waited_for_once_and_not_piled_up() {
        use crate::ssh_test_server::{connect, TestServer};
        // A jump host that never answers the open.
        let stalled = Arc::new(connect(TestServer::stalling_direct_tcpip(), |_| {}, russh::client::Config::default()).await.unwrap());
        let started = Instant::now();
        let first = open_shared(-2, Arc::clone(&stalled), "target", 22, Duration::from_millis(200)).await;
        assert_eq!(first, Through::NotYet, "the page hears in time");
        assert!(started.elapsed() < Duration::from_secs(2));
        // Still waited for in its task: a second check doesn't send another.
        assert!(pending().lock().unwrap().contains(&-2));
        assert_eq!(open_shared(-2, Arc::clone(&stalled), "target", 22, Duration::from_millis(50)).await, Through::NotYet);
        // The task holds its own reference to the connection, not a lock.
        assert!(Arc::strong_count(&stalled) >= 2);
    }

    #[tokio::test]
    async fn a_listening_port_answers_and_a_closed_one_is_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let reply = from_reach(port, crate::diagnose::reach("127.0.0.1", port, CHECK_TIMEOUT).await);
        assert!(matches!(reply, PingReply::Answers { .. }), "{reply:?}");
        drop(listener);
        let reply = from_reach(port, crate::diagnose::reach("127.0.0.1", port, CHECK_TIMEOUT).await);
        assert_eq!(reply, PingReply::Refused { port });
    }
}

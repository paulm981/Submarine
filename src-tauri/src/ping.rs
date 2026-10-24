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
//! connection Submarine already has to it (a tab open on the jump host, or
//! one on another server behind it), with a `direct-tcpip` channel that's
//! closed at once. Without one, the check says so instead of logging in
//! anywhere. A server reached through a proxy or a ProxyCommand isn't checked
//! from here either: a connect from here wouldn't go the way its logins do.

use crate::diagnose::Reach;
use crate::ssh_manager::SshState;
use crate::{ConfigTarget, ConnectRoute, DbState};
use russh::client::Handle;
use serde::Serialize;
use std::collections::HashSet;
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
    /// The jump host's name, None when it's gone.
    jump_name: Option<String>,
    jump_exists: bool,
}

fn read_server(db: &DbState, server_id: i32) -> Result<Server, String> {
    let guard = db.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
    let conn = guard.as_ref().ok_or("Database is not open")?;
    conn.query_row(
        "SELECT s.name, s.host, s.port,
                CASE WHEN COALESCE(s.auth_type, 'vault') = 'vault' THEN c.username ELSE s.username END,
                s.protocol, s.proxy_type, s.proxy_host, s.proxy_port, s.jump_host_id, j.name, j.id
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
                jump_exists: r.get::<_, Option<i32>>(10)?.is_some(),
            })
        },
    )
    .map_err(|_| "Server not found".to_string())
}

/// The servers saved with jump host `jump_id`.
fn servers_behind(db: &DbState, jump_id: i32) -> HashSet<i32> {
    let Ok(guard) = db.conn.lock() else { return HashSet::new() };
    let Some(conn) = guard.as_ref() else { return HashSet::new() };
    let Ok(mut stmt) = conn.prepare("SELECT id FROM servers WHERE jump_host_id = ?1") else { return HashSet::new() };
    stmt.query_map([jump_id], |r| r.get::<_, i32>(0))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

/// A `direct-tcpip` channel through `via` to `host:port`, closed at once.
async fn open_through<H: russh::client::Handler>(via: &Handle<H>, host: &str, port: u16, limit: Duration) -> Result<u64, String> {
    let started = Instant::now();
    let open = via.channel_open_direct_tcpip(host.to_string(), u32::from(port), "127.0.0.1", 0);
    match tokio::time::timeout(limit, open).await {
        Ok(Ok(channel)) => {
            let ms = started.elapsed().as_millis() as u64;
            let _ = channel.close().await;
            Ok(ms)
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(String::new()),
    }
}

/// Check `host:port` from jump host `jump_id`, over a connection Submarine
/// already has to it: None when there's none.
async fn check_through_jump(
    ssh: &SshState,
    db: &DbState,
    jump_id: i32,
    host: &str,
    port: u16,
) -> Option<Result<u64, String>> {
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let left = || deadline.saturating_duration_since(Instant::now());
    let sessions: Vec<(String, i32)> = ssh.session_servers.lock().await.iter().map(|(s, id)| (s.clone(), *id)).collect();

    // A tab open on the jump host itself.
    let own: Vec<_> = {
        let conns = ssh.connections.lock().await;
        sessions.iter().filter(|(_, id)| *id == jump_id).filter_map(|(s, _)| conns.get(s).cloned()).collect()
    };
    for handle in own {
        let Ok(h) = tokio::time::timeout(left(), handle.lock()).await else { break };
        if !h.is_closed() {
            return Some(open_through(&h, host, port, left()).await);
        }
    }

    // A tab on another server behind it: its route ends at the jump host.
    let behind = servers_behind(db, jump_id);
    let routes: Vec<String> = sessions.iter().filter(|(_, id)| behind.contains(id)).map(|(s, _)| s.clone()).collect();
    if routes.is_empty() {
        return None;
    }
    let Ok(jumps) = tokio::time::timeout(left(), ssh.jump_connections.lock()).await else { return None };
    let via = routes
        .iter()
        .filter_map(|s| jumps.get(s).and_then(|route| route.last()))
        .find(|h| !h.is_closed())?;
    Some(open_through(via, host, port, left()).await)
}

/// Check `server_id` once: does its port take a connection, and how fast.
#[tauri::command]
pub async fn ping_server(
    ssh: tauri::State<'_, SshState>,
    db: tauri::State<'_, DbState>,
    server_id: i32,
) -> Result<PingReply, String> {
    let s = read_server(&db, server_id)?;
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
            if !s.jump_exists {
                return Ok(PingReply::NotChecked { why: "Its jump host is gone: edit the server to pick another.".into() });
            }
            let through = s.jump_name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "its jump host".into());
            match check_through_jump(&ssh, &db, jump_id, &target.host, port).await {
                Some(Ok(ms)) => PingReply::Answers { port, ms, through: Some(through) },
                Some(Err(e)) => PingReply::NoAnswer { port, why: Some(e).filter(|e| !e.is_empty()), through: Some(through) },
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
        let jump = connect(TestServer::default(), |_| {}, russh::client::Config::default()).await.unwrap();
        assert!(open_through(&jump, "target", 22, CHECK_TIMEOUT).await.is_ok());
        // A jump host that never answers: no answer, within the limit.
        let stalled = connect(TestServer::stalling_direct_tcpip(), |_| {}, russh::client::Config::default()).await.unwrap();
        let started = Instant::now();
        assert_eq!(open_through(&stalled, "target", 22, Duration::from_millis(200)).await, Err(String::new()));
        assert!(started.elapsed() < Duration::from_secs(2));
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

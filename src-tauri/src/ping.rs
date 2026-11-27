//! Ping a server from the session list (#33): is it up, and does its port
//! (SSH's, or the one its protocol uses) take a connection yet, while it
//! reboots, say.
//!
//! One check per call, no task left behind (but the hold below): the panel
//! asks every couple of seconds while it's open and stops asking when it
//! closes. A check is a plain TCP connect, so it needs no privileges (ICMP
//! would) and works where ping is blocked. Nothing is sent and nobody logs
//! in: the connection is closed as soon as it's made.
//!
//! A server behind a saved jump host is checked from the jump host, over a
//! connection Submarine already has to it (the route of a tab on another
//! server behind it, or a tab open on the jump host itself), with a
//! `direct-tcpip` channel that's closed as soon as it opens, even when the
//! jump host answers after the page has stopped waiting. One such check per
//! server is in flight at a time, and none holds a lock other tabs need. Without one, the check says so instead of logging in
//! anywhere. A server reached through a proxy or a ProxyCommand isn't checked
//! from here either: a connect from here wouldn't go the way its logins do.
//!
//! A tab's route through a jump host closes when the tab's connection drops
//! (the original project's #388). So a server that reboots, and whose own tab
//! was the only one through that jump host, couldn't be checked any more.
//! Ping keeps its own hold on the route it last checked through (`Holds`),
//! the way the route itself holds its jump host logins: a reference to each
//! connection, and a lease on each login shared with other tabs
//! (jump_pool.rs). The hold lasts `HOLD_FOR` past the last check, so it goes
//! soon after the panel closes, and goes at once when the profile is locked.
//! It's never used in another profile.

use crate::diagnose::Reach;
use crate::jump_pool::Lease;
use crate::ssh_manager::{ClientHandler, SshState};
use crate::{ConfigTarget, ConnectRoute, DbState};
use russh::client::{Handle, Handler};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
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

/// Through a tab's own connection to the jump host, which the tab uses too
/// (its channels open side by side, SshState::connections), for no longer
/// than `limit`.
async fn open_on_tab(
    server_id: i32,
    tab: &tokio::sync::RwLock<Handle<ClientHandler>>,
    host: &str,
    port: u16,
    limit: Duration,
) -> Option<Through> {
    if tab_cooldowns().lock().ok()?.get(&server_id).is_some_and(|until| Instant::now() < *until) {
        return Some(Through::NotYet);
    }
    let h = tab.try_read().ok()?;
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

/// How long Ping keeps its hold on a jump host after its last check through
/// it. The panel checks every 2 s, and every 30 s once the server answers
/// (PingPanel.tsx): a little more than that, so a server that goes down
/// between two checks is still checked through it.
const HOLD_FOR: Duration = Duration::from_secs(45);

/// Ping's own hold on a jump host's route: its connections, the jump host's
/// last, and a lease on each login it shares with other tabs.
struct Hold<H: Handler + 'static> {
    /// The profile it was taken in (DbState::generation_now): a server id is
    /// another server in the next profile, so it's never used there.
    opened: u64,
    /// Which hold this is, for the task that lets it go (`let_go_when_idle`).
    serial: u64,
    handles: Vec<Arc<Handle<H>>>,
    _leases: Vec<Lease<H>>,
    until: tokio::time::Instant,
}

/// Ping's holds, one per jump host (its saved server's id).
pub(crate) struct Holds<H: Handler + 'static> {
    map: std::sync::Mutex<HashMap<i32, Hold<H>>>,
    serial: AtomicU64,
}

/// The app's holds, on routes of SSH tabs.
static HOLDS: LazyLock<Arc<Holds<ClientHandler>>> = LazyLock::new(Holds::new);

/// The profile closed (`SshState::forget_fork_sessions`): every hold goes,
/// and with it the connections and logins only Ping still used.
pub(crate) fn let_go_all() {
    HOLDS.clear();
}

impl<H: Handler + 'static> Holds<H> {
    fn new() -> Arc<Self> {
        Arc::new(Holds { map: std::sync::Mutex::new(HashMap::new()), serial: AtomicU64::new(0) })
    }

    /// Hold `handles` (a route through `jump_id`, the jump host's
    /// connection last) and a new lease on each of `leases`, for HOLD_FOR
    /// from now, and return the jump host's connection. The route held
    /// already is only renewed. A route in another profile replaces it.
    fn hold(self: &Arc<Self>, jump_id: i32, opened: u64, handles: &[Arc<Handle<H>>], leases: &[Lease<H>]) -> Option<Arc<Handle<H>>> {
        let last = Arc::clone(handles.last()?);
        let until = tokio::time::Instant::now() + HOLD_FOR;
        let renewed = self.map.lock().ok().is_some_and(|mut map| match map.get_mut(&jump_id) {
            Some(h) if h.opened == opened && h.handles.last().is_some_and(|l| Arc::ptr_eq(l, &last)) => {
                h.until = until;
                true
            }
            _ => false,
        });
        if renewed {
            return Some(last);
        }
        // Taken outside the holds' lock: a lease takes the pool's.
        let leases: Vec<Lease<H>> = leases.iter().filter_map(Lease::again).collect();
        let (replaced, serial) = {
            let Ok(mut map) = self.map.lock() else { return Some(last) };
            let serial = match map.get(&jump_id) {
                Some(h) => h.serial,
                None => self.serial.fetch_add(1, Ordering::Relaxed) + 1,
            };
            let hold = Hold { opened, serial, handles: handles.to_vec(), _leases: leases, until };
            (map.insert(jump_id, hold), serial)
        };
        // The one it replaces is let go outside the lock (its leases lock
        // the pool); its task goes on for this one.
        match replaced {
            Some(old) => drop(old),
            None => self.let_go_when_idle(jump_id, serial),
        }
        Some(last)
    }

    /// The jump host's connection Ping holds for `jump_id` in this profile,
    /// renewed for HOLD_FOR: the route it checked through before, which may
    /// have closed for its tab since. None when there's none, or it closed.
    fn held(&self, jump_id: i32, opened: u64) -> Option<Arc<Handle<H>>> {
        let gone = {
            let mut map = self.map.lock().ok()?;
            let h = map.get_mut(&jump_id)?;
            match h.handles.last() {
                Some(last) if h.opened == opened && !last.is_closed() => {
                    h.until = tokio::time::Instant::now() + HOLD_FOR;
                    return Some(Arc::clone(last));
                }
                _ => map.remove(&jump_id),
            }
        };
        drop(gone);
        None
    }

    /// Lets the hold numbered `serial` go once it hasn't been used for
    /// HOLD_FOR: one task per jump host held, which ends with its hold.
    fn let_go_when_idle(self: &Arc<Self>, jump_id: i32, serial: u64) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let me = Arc::clone(self);
        rt.spawn(async move {
            loop {
                let until = {
                    let Ok(map) = me.map.lock() else { return };
                    match map.get(&jump_id) {
                        Some(h) if h.serial == serial => h.until,
                        _ => return,
                    }
                };
                tokio::time::sleep_until(until).await;
                let gone = {
                    let Ok(mut map) = me.map.lock() else { return };
                    let idle = map.get(&jump_id).is_some_and(|h| h.serial == serial && h.until <= tokio::time::Instant::now());
                    if idle { map.remove(&jump_id) } else { None }
                };
                if gone.is_some() {
                    drop(gone);
                    return;
                }
            }
        });
    }

    fn clear(&self) {
        let gone = self.map.lock().map(|mut map| std::mem::take(&mut *map)).unwrap_or_default();
        drop(gone);
    }
}

/// Check `host:port` from jump host `jump_id`, over a connection Submarine
/// already has to it: None when there's none. A route a tab behind it opened
/// is used first (only one that went through this jump host), held by Ping
/// for its next checks; then the route Ping held from before, whose tab's
/// connection may have dropped since (the server rebooting); then a tab on
/// the jump host itself. No lock is held while the channel opens. `opened`:
/// the open profile (DbState::generation_now).
async fn check_through_jump(ssh: &SshState, opened: u64, server_id: i32, jump_id: i32, host: &str, port: u16) -> Option<Through> {
    let route = {
        let routes = ssh.jump_connections.lock().await;
        routes
            .values()
            .filter(|r| r.jump_id == Some(jump_id))
            .find(|r| r.handles.last().is_some_and(|h| !h.is_closed()))
            .and_then(|r| HOLDS.hold(jump_id, opened, &r.handles, &r.leases))
    };
    if let Some(via) = route.or_else(|| HOLDS.held(jump_id, opened)) {
        return Some(open_shared(server_id, via, host, port, CHECK_TIMEOUT).await);
    }
    let sessions: Vec<String> =
        ssh.fork.session_servers.lock().await.iter().filter(|(_, id)| **id == jump_id).map(|(s, _)| s.clone()).collect();
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
            match check_through_jump(&ssh, db.generation_now(), server_id, jump_id, &target.host, port).await {
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
            closing: Default::default(),
            saved_at: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(crate::NOT_SAVED)),
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

    /// A Lock lets Ping's holds go: teardown_profile calls
    /// forget_fork_sessions (checked in root_state.rs), which calls this.
    #[test]
    fn a_lock_lets_the_holds_go() {
        crate::fork_hooks::assert_hooks(
            "root_state.rs",
            include_str!("ssh_manager/root_state.rs"),
            &[("forget_fork_sessions", "crate::ping::let_go_all()", 1)],
        );
    }

    /// A login a tab shares with others (jump_pool.rs), made in `pool`, and
    /// its key.
    async fn shared_login(
        pool: &Arc<crate::jump_pool::Pool<crate::ssh_test_server::TestClient>>,
        jump: &Arc<Handle<crate::ssh_test_server::TestClient>>,
    ) -> (crate::jump_pool::Key, Lease<crate::ssh_test_server::TestClient>) {
        use crate::jump_pool::{digest, Acquired, Key, Via};
        let key = Key::new(Via::Direct, "bastion:22".into(), "alice".into(), "jump5".into(), digest(&[b"pw"]));
        let gens = Arc::new(tokio::sync::Mutex::new(HashMap::from([("session-1".to_string(), 1)])));
        let attempt = crate::ssh_manager::ConnectAttempt::new(gens, "session-1".into(), 1);
        let Ok(Acquired::Create(reservation)) = pool.acquire(key.clone(), "session-1", &attempt, 8, || {}).await else {
            panic!("the first tab makes the login")
        };
        let lease = reservation.ready(Arc::clone(jump), None, None).expect("the pool keeps it");
        (key, lease)
    }

    /// The server behind the jump host reboots, and its tab was the only one
    /// through it: the tab's route closes with its connection (#388), but
    /// Ping's hold keeps the jump host's connection, and its share of the
    /// login, so the server is still checked through it. Never in another
    /// profile.
    #[tokio::test]
    async fn a_route_ping_held_still_checks_after_its_tab_dropped() {
        use crate::ssh_test_server::{connect, TestServer};
        let jump = Arc::new(connect(TestServer::default(), |_| {}, russh::client::Config::default()).await.unwrap());
        let pool = crate::jump_pool::Pool::new();
        let (key, route_lease) = shared_login(&pool, &jump).await;
        let holds = Holds::new();
        // A check through the tab's route holds it.
        let via = holds.hold(5, 1, &[Arc::clone(&jump)], std::slice::from_ref(&route_lease)).unwrap();
        assert!(Arc::ptr_eq(&via, &jump));
        assert_eq!(pool.census(&key), vec![2], "Ping has a share of the login, beside the tab's");
        // The next check through the same route only renews it.
        holds.hold(5, 1, &[Arc::clone(&jump)], std::slice::from_ref(&route_lease)).unwrap();
        assert_eq!(pool.census(&key), vec![2]);
        // The tab's connection drops: its route goes, with its lease.
        drop((via, route_lease));
        let alive = Arc::downgrade(&jump);
        drop(jump);
        assert_eq!(pool.census(&key), vec![1], "the login stays shared, Ping still uses it");
        let held = holds.held(5, 1).expect("Ping still holds the route");
        assert!(!held.is_closed());
        assert!(matches!(open_shared(-3, held, "target", 22, CHECK_TIMEOUT).await, Through::Answers(_)));
        // Another profile never uses it, and it goes.
        assert!(holds.held(5, 2).is_none());
        assert!(holds.held(5, 1).is_none());
        assert_eq!(pool.census(&key), vec![0]);
        // The pool's own reference lingers (LINGER) for a tab that comes back.
        assert!(alive.upgrade().is_some());
    }

    /// The hold lasts HOLD_FOR past the last check, renewed by each, so it
    /// goes soon after the panel closes; a Lock lets every hold go at once.
    #[tokio::test(start_paused = true)]
    async fn a_hold_goes_when_ping_stops_and_at_a_lock() {
        use crate::ssh_test_server::{connect, TestServer};
        let jump = Arc::new(connect(TestServer::default(), |_| {}, russh::client::Config::default()).await.unwrap());
        let alive = Arc::downgrade(&jump);
        let holds = Holds::new();
        holds.hold(5, 1, &[jump], &[]).unwrap();
        tokio::time::sleep(HOLD_FOR - Duration::from_secs(15)).await;
        assert!(holds.held(5, 1).is_some(), "a check renews it");
        tokio::time::sleep(HOLD_FOR - Duration::from_secs(5)).await;
        assert!(holds.map.lock().unwrap().contains_key(&5), "still held: checked less than HOLD_FOR ago");
        tokio::time::sleep(Duration::from_secs(10)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(holds.map.lock().unwrap().is_empty(), "let go once not checked for HOLD_FOR");
        assert!(alive.upgrade().is_none(), "and its connection with it");
        // A Lock.
        let jump = Arc::new(connect(TestServer::default(), |_| {}, russh::client::Config::default()).await.unwrap());
        let alive = Arc::downgrade(&jump);
        holds.hold(7, 1, &[jump], &[]).unwrap();
        holds.clear();
        assert!(alive.upgrade().is_none());
        assert!(holds.held(7, 1).is_none());
    }

    #[tokio::test]
    async fn a_listening_port_answers_and_a_closed_one_is_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let reply = from_reach(port, crate::diagnose::reach("127.0.0.1", port, CHECK_TIMEOUT).await);
        assert!(matches!(reply, PingReply::Answers { .. }), "{reply:?}");
        drop(listener);
        let reply = from_reach(port, crate::diagnose::reach("127.0.0.1", port, CHECK_TIMEOUT).await);
        if cfg!(windows) {
            // Windows retries a refused connect for about 2 s, longer than a
            // check waits, so a closed port there reads as no answer.
            assert!(
                matches!(reply, PingReply::Refused { .. } | PingReply::NoAnswer { why: None, .. }),
                "{reply:?}"
            );
        } else {
            assert_eq!(reply, PingReply::Refused { port });
        }
    }
}

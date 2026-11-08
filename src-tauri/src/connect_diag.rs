//! How a connection reaches its target (`connect_route`), and the
//! connect diagnosis for Quick Connect (`diagnose_connect`).
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use connect_diag::*`.

use crate::*;

/// A saved port as one a connection can use. The form only takes 1 to 65535,
/// but a synced or imported server may carry anything, and a plain cast would
/// quietly dial another port.
pub(crate) fn valid_port(port: i32) -> Result<u16, String> {
    u16::try_from(port).map_err(|_| format!("Port {port} isn't a valid port"))
}

impl ConfigTarget {
    /// A jump host as `connect_jump_host` reaches it, for the connect, the
    /// check under a failed one and the monitor alike: its saved port
    /// checked, `~/.ssh/config` applied, and the ProxyCommand or ProxyJump
    /// the config gives it (a jump host is reached without its own proxy
    /// setting, so the config's has nothing to give way to).
    pub(crate) async fn resolve_jump_host(
        host: &str,
        port: i32,
        user: &str,
        name: Option<String>,
    ) -> Result<(Self, Option<ssh_config::Proxy>), String> {
        let mut target = Self::resolve(host, valid_port(port)?, user, name).await;
        let proxy = target.proxy(None);
        Ok((target, proxy))
    }
}

/// How a connection reaches its target. Chosen in one place for the connect
/// and for the check under a failed one (`diagnose_connect`), so the check
/// looks at what the connect dials.
#[derive(Debug, PartialEq)]
pub(crate) enum ConnectRoute {
    /// Through the server's jump host (its id).
    Jump(i32),
    /// Through the server's SOCKS5 or HTTP proxy. A missing host is the
    /// connect's error to report; a missing port is the usual one.
    Proxy { socks5: bool, host: Option<String>, port: u16 },
    /// Through the ProxyCommand ~/.ssh/config gives the host.
    Command(String),
    /// Through the ProxyJump chain ~/.ssh/config gives the host.
    ConfigJump(String),
    /// Straight there.
    Direct,
}

/// The route for a server saved with this jump host and proxy, and the
/// ~/.ssh/config `target` its address resolved to. The server's own jump
/// host or proxy wins over the config's (`ConfigTarget::proxy` logs that).
pub(crate) fn connect_route(
    jump_host_id: Option<i32>,
    proxy_type: &str,
    proxy_host: Option<String>,
    proxy_port: Option<i32>,
    target: &mut ConfigTarget,
) -> ConnectRoute {
    let own_route = if jump_host_id.is_some() {
        Some("jump host")
    } else {
        match proxy_type {
            "socks5" => Some("SOCKS5 proxy"),
            "http" => Some("HTTP proxy"),
            _ => None,
        }
    };
    let config_proxy = target.proxy(own_route);
    if let Some(id) = jump_host_id {
        return ConnectRoute::Jump(id);
    }
    let host = proxy_host.filter(|h| !h.is_empty());
    match proxy_type {
        "socks5" => return ConnectRoute::Proxy { socks5: true, host, port: proxy_port.map_or(1080, |p| p as u16) },
        "http" => return ConnectRoute::Proxy { socks5: false, host, port: proxy_port.map_or(8080, |p| p as u16) },
        _ => {}
    }
    match config_proxy {
        Some(ssh_config::Proxy::Command(command)) => ConnectRoute::Command(command),
        Some(ssh_config::Proxy::Jump(spec)) => ConnectRoute::ConfigJump(spec),
        _ => ConnectRoute::Direct,
    }
}

/// A Quick Connect target, for `diagnose_connect`: where it goes and as whom,
/// without the password or key the connect itself is sent.
#[derive(serde::Deserialize)]
pub(crate) struct QuickTarget {
    pub(crate) host: String,
    pub(crate) port: i32,
    #[serde(default)]
    pub(crate) username: String,
}

/// What a failed connect to server `server_id` (or a Quick Connect target)
/// can say about why: whether the name resolves and the port answers, at a
/// proxy's or jump host's address when there is one (see `diagnose`). The tab
/// runs it beside the retry countdown and shows the lines under the error.
/// The route is the connect's own (`connect_route`), and ~/.ssh/config is
/// asked with the same name and user.
#[tauri::command]
pub(crate) async fn diagnose_connect(
    db_state: tauri::State<'_, DbState>,
    server_id: i32,
    quick_target: Option<QuickTarget>,
) -> Result<Vec<String>, String> {
    // The user a server logs in as: its credential's in vault mode, its own
    // otherwise (as in initiate_connection and resolve_jump_auth).
    const USER: &str = "CASE WHEN COALESCE({s}.auth_type, 'vault') = 'vault' THEN {c}.username ELSE {s}.username END";
    struct Hop { name: Option<String>, host: String, port: i32, user: Option<String> }
    struct Row {
        name: Option<String>, host: String, port: i32, user: Option<String>,
        proxy_type: String, proxy_host: Option<String>, proxy_port: Option<i32>,
        jump_id: Option<i32>, jump: Option<Hop>,
    }
    let row = if let Some(q) = quick_target {
        // Quick Connect has no name, proxy or jump host.
        Row {
            name: None, host: q.host, port: q.port, user: Some(q.username),
            proxy_type: "none".into(), proxy_host: None, proxy_port: None, jump_id: None, jump: None,
        }
    } else {
        let guard = db_state.conn.lock().map_err(|_| "[STATE] LOCK_CONN")?;
        let conn = guard.as_ref().ok_or("Database is not open")?;
        let sql = format!(
            "SELECT s.name, s.host, s.port, {}, s.proxy_type, s.proxy_host, s.proxy_port, s.jump_host_id,
                    j.name, j.host, j.port, {}
             FROM servers s LEFT JOIN credentials c ON c.id = s.credential_id
             LEFT JOIN servers j ON j.id = s.jump_host_id LEFT JOIN credentials jc ON jc.id = j.credential_id
             WHERE s.id=?1",
            USER.replace("{s}", "s").replace("{c}", "c"),
            USER.replace("{s}", "j").replace("{c}", "jc"),
        );
        conn.query_row(&sql, [server_id], |r| {
            let jump = match (r.get::<_, Option<String>>(9)?, r.get::<_, Option<i32>>(10)?) {
                (Some(host), Some(port)) => Some(Hop { name: r.get(8)?, host, port, user: r.get(11)? }),
                _ => None,
            };
            Ok(Row {
                name: r.get(0)?,
                host: r.get(1)?,
                port: r.get(2)?,
                user: r.get(3)?,
                proxy_type: r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "none".into()),
                proxy_host: r.get(5)?,
                proxy_port: r.get(6)?,
                jump_id: r.get(7)?,
                jump,
            })
        })
        .map_err(|_| "Server not found".to_string())?
    };
    let port = valid_port(row.port)?;

    // The address an alias in ~/.ssh/config stands for, as the connect uses it.
    let mut target = ConfigTarget::resolve(&row.host, port, row.user.as_deref().unwrap_or(""), row.name).await;
    let route = match connect_route(row.jump_id, &row.proxy_type, row.proxy_host, row.proxy_port, &mut target) {
        ConnectRoute::Jump(_) => {
            // The connect fails on its own when the jump host is gone.
            let Some(hop) = row.jump else { return Ok(Vec::new()) };
            let label = hop.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| hop.host.clone());
            let (resolved, proxy) =
                ConfigTarget::resolve_jump_host(&hop.host, hop.port, hop.user.as_deref().unwrap_or(""), hop.name).await?;
            // connect_jump_host takes the jump host's ProxyCommand or
            // ProxyJump when it has one.
            match proxy {
                Some(ssh_config::Proxy::Command(_)) => diagnose::Route::Command,
                Some(ssh_config::Proxy::Jump(spec)) => match first_hop_route(&spec, 1).await {
                    Some(route) => route,
                    None => return Ok(Vec::new()),
                },
                _ => diagnose::Route::Jump { name: label, host: resolved.host, port: resolved.port },
            }
        }
        ConnectRoute::Proxy { socks5, host: Some(host), port } => {
            diagnose::Route::Proxy { kind: if socks5 { "SOCKS5" } else { "HTTP" }, host, port }
        }
        // No proxy address to check: the connect says the proxy host is empty.
        ConnectRoute::Proxy { host: None, .. } => return Ok(Vec::new()),
        ConnectRoute::Command(_) => diagnose::Route::Command,
        ConnectRoute::ConfigJump(spec) => match first_hop_route(&spec, 0).await {
            Some(route) => route,
            None => return Ok(Vec::new()),
        },
        ConnectRoute::Direct => diagnose::Route::Direct,
    };
    Ok(diagnose::diagnose(&target.host, target.port, route).await)
}

/// What `diagnose_connect` checks for a route through the ProxyJump `spec`:
/// the first hop the connect dials (`hop_policy::first_hop`), with `depth`
/// hops already in the route (1 behind a saved jump host). `None` when the
/// list can't be used; the connect says why.
pub(crate) async fn first_hop_route(spec: &str, depth: usize) -> Option<diagnose::Route> {
    match hop_policy::first_hop_in_config(spec, depth).await.ok()? {
        hop_policy::FirstHop::Address { label, host, port } => Some(diagnose::Route::Jump { name: label, host, port }),
        hop_policy::FirstHop::Command => Some(diagnose::Route::Command),
    }
}

#[cfg(test)]
mod connect_route_tests {
    use super::*;

    /// `web` as ~/.ssh/config would give it, with this proxy line.
    fn target(proxy: Option<ssh_config::Proxy>) -> ConfigTarget {
        ConfigTarget::apply("web", 22, "", Some(ssh_config::HostConfig { proxy, ..Default::default() }))
    }

    fn command() -> Option<ssh_config::Proxy> {
        Some(ssh_config::Proxy::Command("nc %h %p".into()))
    }

    #[test]
    fn a_port_out_of_range_is_refused_not_cast() {
        assert_eq!(valid_port(22), Ok(22));
        assert_eq!(valid_port(65535), Ok(65535));
        // 65558 as u16 would have dialled port 22.
        assert_eq!(valid_port(65558), Err("Port 65558 isn't a valid port".to_string()));
        assert_eq!(valid_port(-1), Err("Port -1 isn't a valid port".to_string()));
    }

    #[test]
    fn a_proxy_without_a_port_takes_the_usual_one() {
        let socks = connect_route(None, "socks5", Some("p".into()), None, &mut target(None));
        assert_eq!(socks, ConnectRoute::Proxy { socks5: true, host: Some("p".into()), port: 1080 });
        let http = connect_route(None, "http", Some("p".into()), None, &mut target(None));
        assert_eq!(http, ConnectRoute::Proxy { socks5: false, host: Some("p".into()), port: 8080 });
        let empty = connect_route(None, "http", Some(String::new()), Some(3128), &mut target(None));
        assert_eq!(empty, ConnectRoute::Proxy { socks5: false, host: None, port: 3128 });
    }

    #[test]
    fn the_servers_own_route_wins_over_a_proxy_command() {
        assert_eq!(connect_route(Some(7), "socks5", Some("p".into()), None, &mut target(command())), ConnectRoute::Jump(7));
        let mut t = target(command());
        assert!(matches!(connect_route(None, "socks5", Some("p".into()), Some(1), &mut t), ConnectRoute::Proxy { .. }));
        assert!(t.log.iter().any(|l| l.contains("ProxyCommand isn't used")), "{:?}", t.log);
    }

    #[test]
    fn otherwise_the_config_decides() {
        assert_eq!(connect_route(None, "none", None, None, &mut target(command())), ConnectRoute::Command("nc %h %p".into()));
        let jump = Some(ssh_config::Proxy::Jump("bastion".into()));
        assert_eq!(connect_route(None, "none", None, None, &mut target(jump)), ConnectRoute::ConfigJump("bastion".into()));
        assert_eq!(connect_route(None, "none", None, None, &mut target(None)), ConnectRoute::Direct);
    }

    // D2: the server's own saved jump host wins over its config ProxyJump,
    // and the log says so; so does its own SOCKS / HTTP proxy.
    #[test]
    fn the_servers_own_route_wins_over_a_config_proxy_jump() {
        let jump = || Some(ssh_config::Proxy::Jump("bastion".into()));
        let mut t = target(jump());
        assert_eq!(connect_route(Some(7), "none", None, None, &mut t), ConnectRoute::Jump(7));
        assert!(t.log.iter().any(|l| l.contains("ProxyJump isn't used: this server goes through its own jump host")), "{:?}", t.log);
        let mut t = target(jump());
        assert!(matches!(connect_route(None, "http", Some("p".into()), None, &mut t), ConnectRoute::Proxy { socks5: false, .. }));
        assert!(t.log.iter().any(|l| l.contains("ProxyJump isn't used: this server goes through its own HTTP proxy")), "{:?}", t.log);
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod connect_diag;", 1),
                ("", "use connect_diag::*;", 1),
                ("initiate_connection", "connect_route(", 1),
                ("run", "diagnose_connect", 1),
                ("initiate_connection", "valid_port(port)?", 1),
                ("resolve_node_auth_row", "valid_port(port)?", 1),
                ("connect_jump_host", "ConfigTarget::resolve_jump_host(", 1),
            ],
        );
    }
}

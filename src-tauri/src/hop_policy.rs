//! The fork's rules for the hops of a jump route, kept out of lib.rs so the
//! hop code there stays the original project's (#169: `connect_jump_host`,
//! `config_jump_chain`, `reach_hop`, `login_hop`), with one-line calls into
//! this file.

use crate::ssh_config::Proxy;
use std::future::Future;
use std::pin::Pin;

/// A `ProxyJump` with `%` tokens (`%h`, `%r`, …) is refused before anything
/// is dialled (#43, D7). OpenSSH fills them in; Submarine doesn't yet, and
/// taken as written they'd send a login to a host literally called `%h`.
/// An IPv6 address with a zone (`[fe80::1%en0]`) has a `%` too, and isn't
/// usable either; it gets its own message.
pub(crate) fn refuse_tokens(spec: &str) -> Result<(), String> {
    let has_zone = spec.split(',').any(|item| {
        let item = item.trim();
        let item = item.strip_prefix("ssh://").unwrap_or(item);
        let host = item.rsplit_once('@').map_or(item, |(_, h)| h);
        // An IPv6 address: in brackets, or bare with more than one colon.
        let v6 = host.strip_prefix('[').and_then(|h| h.split_once(']')).map(|(h, _)| h).unwrap_or(host);
        v6.matches(':').count() >= 2 && v6.contains('%')
    });
    if has_zone {
        return Err(format!(
            "ProxyJump {spec:?} in ~/.ssh/config names an IPv6 address with a zone (the part after %), \
             which Submarine can't use there. Use a host name or an address without the zone."
        ));
    }
    if spec.contains('%') {
        return Err(format!(
            "ProxyJump {spec:?} in ~/.ssh/config uses % tokens (such as %h or %r), which Submarine doesn't fill in yet. \
             Write the jump hosts out in full."
        ));
    }
    Ok(())
}

/// The login queue a jump host dialled from here waits in
/// (multiexec::login_turn): its address, so a saved jump host and the same
/// bastion in a ProxyJump chain share one queue, as Monitoring does (#43, D1).
pub(crate) fn queue_key(host: &str, port: u16) -> String {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    format!("[{host}]:{port}")
}

/// The login queues of one route's hops, in the order they log in, and, for
/// an SSH terminal tab, the jump host logins it shares (jump_pool.rs, #237).
#[derive(Default)]
pub(crate) struct RouteQueues {
    /// The queue of the hop that logged in last: the one the next hop is
    /// reached through, since hops log in one after another along the route.
    last: std::sync::Mutex<Option<String>>,
    /// `None`: every hop logs in for this route alone (sharing off, or not
    /// an SSH terminal tab).
    share: Option<Share>,
}

/// A route's part in the pool of shared logins.
#[derive(Default)]
struct Share {
    /// The pool login of the hop the next one is reached through, if the
    /// last hop's login is one.
    last_login: std::sync::Mutex<Option<u64>>,
    /// The login this route is making now, for `login_hop` to tune, and,
    /// once it's logged in, the login and a lease on the login of the hop it
    /// is reached through (`keep`), for `Publish` to put in the pool.
    making: std::sync::Mutex<Option<crate::jump_pool::Reservation<crate::ssh_manager::ClientHandler>>>,
    made: std::sync::Mutex<Option<(HopHandle, Option<crate::jump_pool::Lease<crate::ssh_manager::ClientHandler>>)>>,
    /// A lease on each pool login of the route, kept with the route
    /// (`take_leases`), the last hop's last.
    leases: std::sync::Mutex<crate::jump_pool::Leases>,
}

fn locked<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A hop's login: shared by the tabs on it (`jump_pool`), or this route's own.
pub(crate) type HopHandle = std::sync::Arc<russh::client::Handle<crate::ssh_manager::ClientHandler>>;

/// A route's hops as a protocol that keeps them to itself has them (RDP,
/// telnet, FTP, VNC): their routes never share a login, so each is this
/// route's only reference.
pub(crate) fn owned(chain: Vec<HopHandle>) -> Result<Vec<russh::client::Handle<crate::ssh_manager::ClientHandler>>, String> {
    chain
        .into_iter()
        .map(|h| std::sync::Arc::try_unwrap(h).map_err(|_| "a jump host login of this route is shared with another tab".to_string()))
        .collect()
}

impl RouteQueues {
    /// The route of `session_id`: its hops share logins with other tabs' when
    /// sharing is on (Settings › Servers & sessions) and it's an SSH terminal
    /// tab's own connection, not its `::sftp`, `::fwd` or a terminal's own
    /// login (`::term-`).
    pub(crate) fn shared(app: &tauri::AppHandle, session_id: &str) -> Self {
        Self::sharing(crate::jump_pool::sharing_on(app), session_id)
    }

    pub(crate) fn sharing(on: bool, session_id: &str) -> Self {
        let share = (on && !session_id.contains("::")).then(Share::default);
        RouteQueues { last: Default::default(), share }
    }

    /// The queue of the next hop, `host:port`. One dialled from here is known
    /// by its address. One reached through the hop before it is known by that
    /// hop's queue and its address: `10.0.0.5` behind one gateway isn't the
    /// same machine as `10.0.0.5` behind another.
    fn next(&self, through_previous: bool, host: &str, port: u16) -> String {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let here = queue_key(host, port);
        let key = match (through_previous, last.as_deref()) {
            (true, Some(prev)) => format!("{prev} > {here}"),
            _ => here,
        };
        *last = Some(key.clone());
        key
    }

    /// The client config of the hop logging in now (login_hop): a login
    /// other tabs will share gets the shared window (`jump_pool::tune`).
    pub(crate) fn tune(&self, config: russh::client::Config) -> russh::client::Config {
        match &self.share {
            Some(share) if locked(&share.making).is_some() => crate::jump_pool::tune(config),
            _ => config,
        }
    }

    /// The hop logged in (login_hop). A login made for the pool goes in it
    /// once the hop's step is over (`Publish`), for the tabs waiting on it.
    pub(crate) fn keep(&self, session: russh::client::Handle<crate::ssh_manager::ClientHandler>) -> HopHandle {
        let login = std::sync::Arc::new(session);
        let Some(share) = &self.share else { return login };
        let mut last = locked(&share.last_login);
        if locked(&share.making).is_none() {
            *last = None;
            return login;
        }
        // Held by the new login as long as it lives: the login of the hop it
        // is reached through.
        let behind = last.and_then(|id| {
            let leases = locked(&share.leases);
            leases.iter().rev().find(|l| l.login_id() == id).and_then(|l| crate::jump_pool::POOL.lease_again(l))
        });
        *locked(&share.made) = Some((std::sync::Arc::clone(&login), behind));
        login
    }

    /// The route's leases, to keep with its handles (ssh_manager::JumpRoute).
    pub(crate) fn take_leases(&self) -> crate::jump_pool::Leases {
        self.share.as_ref().map(|s| std::mem::take(&mut *locked(&s.leases))).unwrap_or_default()
    }
}

/// How the next hop is reached, for `hop_turn`.
pub(crate) enum Via<'a> {
    /// Dialled from here.
    Direct,
    /// Through this ProxyCommand.
    Command(&'a str),
    /// Through the hop before it.
    Previous,
}

/// What `hop_turn` gives a hop.
pub(crate) enum HopTurn<'a> {
    /// Another tab's login to the same jump host, to open the channel on.
    Shared(HopHandle),
    /// This route logs in itself: its turn in the login queue (`None` with no
    /// limit), and, for a login other tabs will share, what tells them how it
    /// went once the hop's login is over. Held, not read: they act when
    /// dropped.
    Login(#[allow(dead_code)] Option<tokio::sync::OwnedSemaphorePermit>, #[allow(dead_code)] Option<Publish<'a>>),
}

/// Tells the tabs waiting on a login this route makes how it went, once the
/// hop's login is over, whatever happened: it is dropped at the end of the
/// hop's step in `reach_hop` / `config_jump_chain`.
pub(crate) struct Publish<'a> {
    ctx: &'a crate::HopCtx<'a>,
    /// Where login_hop keeps a password typed for the hop.
    typed_at: String,
}

impl Drop for Publish<'_> {
    fn drop(&mut self) {
        let Some(share) = &self.ctx.queues.share else { return };
        let Some(mut reservation) = locked(&share.making).take() else { return };
        let mut last = locked(&share.last_login);
        *last = None;
        match locked(&share.made).take() {
            // Logged in: the tabs waiting share it. A password typed for it
            // goes with it, in memory, for a second login of the same key
            // (never a code: only a password is kept in that slot).
            Some((login, behind)) => {
                let typed = self.ctx.cache.try_lock().ok().and_then(|c| c.get(&self.typed_at).and_then(|s| s.jump_password.clone()));
                if let Some(lease) = reservation.ready(login, behind, typed) {
                    *last = Some(lease.login_id());
                    locked(&share.leases).push(lease);
                }
            }
            None => {
                let flag = |f: &std::sync::atomic::AtomicBool| f.load(std::sync::atomic::Ordering::Relaxed);
                reservation.fail(outcome_of(self.ctx.attempt.is_current_now(), flag(self.ctx.auth_failed), flag(self.ctx.network_failed)));
            }
        }
    }
}

/// How a login that didn't finish ended, for the tabs waiting on it: its own
/// tab was closed or reconnected (a waiting tab makes it instead), it was
/// refused or cancelled (they stop too, as an auth error, instead of each
/// asking again), or the network failed (they each try again).
pub(crate) fn outcome_of(current: bool, auth_failed: bool, network_failed: bool) -> crate::jump_pool::Outcome {
    use crate::jump_pool::Outcome;
    match (current, auth_failed, network_failed) {
        (false, _, _) => Outcome::Abandoned,
        (true, true, _) => Outcome::Failed { auth: true, retry: false },
        (true, false, true) => Outcome::Failed { auth: false, retry: true },
        (true, false, false) => Outcome::Abandoned,
    }
}

/// The pool's key for `hop` reached `via`: who logs in where, the way there,
/// the saved jump host (or ~/.ssh/config hop) it is, and a digest of what it
/// logs in with and of its ~/.ssh/config. `None` when it isn't shared: its
/// user is asked at login (#54), or the hop before it isn't in the pool.
pub(crate) fn pool_key(hop: &crate::Hop, via: Option<crate::jump_pool::Via>, use_agent: bool) -> Option<crate::jump_pool::Key> {
    let via = via?;
    let user = hop.target.user.trim();
    if user.is_empty() {
        return None;
    }
    let config = hop.target.config.as_ref().map(|c| {
        // Everything the config says about the login, not the notes it logs.
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            c.hostname, c.port, c.user, c.identity_agent, c.identity_files, c.certificate_files, c.identities_only, c.proxy, c.user_known_hosts_files, c.host_key_algorithms
        )
    });
    let (key, passphrase, key_name) = match &hop.key_data {
        Some((k, p, n)) => (Some(k.as_str()), p.as_deref(), n.as_deref()),
        None => (None, None, None),
    };
    // Each part says whether it's there, so no password and an empty one differ.
    let part = |s: Option<&str>| -> zeroize::Zeroizing<Vec<u8>> {
        zeroize::Zeroizing::new(s.map_or(vec![0], |s| [&[1u8][..], s.as_bytes()].concat()))
    };
    let cred = crate::jump_pool::digest(&[
        &part(hop.password.as_deref()),
        &part(key),
        &part(passphrase),
        &part(key_name),
        &part(config.as_deref()),
        hop.target.original.as_bytes(),
        &[u8::from(use_agent)],
    ]);
    Some(crate::jump_pool::Key::new(via, queue_key(&hop.target.host, hop.target.port), user.to_string(), hop.cache_id.clone(), cred))
}

/// A turn to log in to `hop`, a hop of `ctx`'s route, from the moment its
/// transport opens until its login is over (drop it then); or, for an SSH
/// terminal tab, another tab's login to the same jump host to share
/// (jump_pool.rs, #237). One hop's turn is never held while the next hop
/// waits for one: two routes through the same bastions in opposite orders
/// would wait on each other for good.
pub(crate) async fn hop_turn<'a>(ctx: &'a crate::HopCtx<'a>, hop: &crate::Hop, via: Via<'_>) -> Result<HopTurn<'a>, String> {
    let target = &hop.target;
    let queue = ctx.queues.next(matches!(via, Via::Previous), &target.host, target.port);
    let host = target.host.clone();
    let turn = |queue: String| async move {
        crate::multiexec::login_turn(ctx.app, &queue, ctx.attempt, |limit| {
            ctx.log(&format!("Waiting for a login slot on the jump host {host} ({limit} logging in at a time)..."), "info")
        })
        .await
    };
    let Some(share) = &ctx.queues.share else { return Ok(HopTurn::Login(turn(queue).await?, None)) };
    let via = match via {
        Via::Direct => Some(crate::jump_pool::Via::Direct),
        Via::Command(c) => Some(crate::jump_pool::Via::Command(c.to_string())),
        Via::Previous => locked(&share.last_login).map(crate::jump_pool::Via::Through),
    };
    let Some(key) = pool_key(hop, via, ctx.use_agent) else {
        *locked(&share.last_login) = None;
        return Ok(HopTurn::Login(turn(queue).await?, None));
    };
    // Where login_hop keeps a password typed for this hop (#30).
    let typed_at = format!("{}|{}|{}:{}|{}", crate::base_session_id(ctx.session_id), hop.cache_id, target.host, target.port, target.user.trim());
    let waiting = || ctx.log(&format!("Waiting for the login to jump host {} in another tab...", target.host), "info");
    match crate::jump_pool::POOL.acquire(key, &typed_at, ctx.attempt, waiting).await {
        Ok(crate::jump_pool::Acquired::Shared(login, lease)) => {
            ctx.log(&format!("Using the login to jump host {} that another tab opened.", target.host), "info");
            *locked(&share.last_login) = Some(lease.login_id());
            locked(&share.leases).push(lease);
            Ok(HopTurn::Shared(login))
        }
        Ok(crate::jump_pool::Acquired::Create(reservation)) => {
            // A second login of the same key: the password typed for the
            // first is there for it. Never a code: only a password is kept.
            let typed = match (reservation.typed.clone(), reservation.typed_at.as_deref()) {
                (Some(pw), _) => Some(pw),
                (None, Some(from)) => crate::cache_get_secret(ctx.cache, from, crate::SecretSlot::JumpPassword).await,
                (None, None) => None,
            };
            if let Some(pw) = typed {
                crate::cache_store_secret(ctx.cache, &typed_at, crate::SecretSlot::JumpPassword, pw).await;
            }
            *locked(&share.making) = Some(reservation);
            *locked(&share.made) = None;
            let publish = Publish { ctx, typed_at };
            Ok(HopTurn::Login(turn(queue).await?, Some(publish)))
        }
        Err(crate::jump_pool::Waited::Failed { auth, retry }) => {
            *locked(&share.last_login) = None;
            if auth {
                ctx.auth_failed.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let msg = format!("the login to jump host {} in another tab failed", target.host);
            Err(if retry { ctx.network_error(msg) } else { msg })
        }
        Err(crate::jump_pool::Waited::Cancelled) => Err("jump host: the connection attempt was cancelled".into()),
    }
}

/// Where a route through a `ProxyJump` list first dials.
#[derive(Debug, PartialEq)]
pub(crate) enum FirstHop {
    /// A hop's address, reached directly. `label` is the name in the list.
    Address { label: String, host: String, port: u16 },
    /// A ProxyCommand the first hop's config gives it.
    Command,
}

/// A hop's address and port after `~/.ssh/config`, and its own proxy line.
pub(crate) type Resolved = (String, u16, Option<Proxy>);
/// Resolves `host`, `port`, `user` of a hop the way `config_jump_chain` does.
pub(crate) type Resolver<'a> = &'a (dyn Fn(String, u16, String) -> Pin<Box<dyn Future<Output = Resolved> + Send>> + Sync);

/// The first hop a route through `spec` dials, found without connecting:
/// the first hop of the list, or the first hop of ITS ProxyJump, and so on,
/// as `config_jump_chain` and `reach_hop` follow them, under the same limit
/// (`depth` counts the hops already in the route). For the check under a
/// failed connect (`diagnose_connect`).
pub(crate) fn first_hop<'a>(
    spec: &'a str,
    depth: usize,
    resolve: Resolver<'a>,
) -> Pin<Box<dyn Future<Output = Result<FirstHop, String>> + Send + 'a>> {
    Box::pin(async move {
        refuse_tokens(spec)?;
        let hops = crate::parse_proxy_jump(spec)?;
        // As config_jump_chain: every hop of this list counts before the
        // first hop's own ProxyJump adds its own.
        let depth = depth + hops.len();
        if depth > crate::MAX_JUMP_HOPS {
            return Err(too_many_hops());
        }
        let Some(first) = hops.into_iter().next() else { return Err(format!("ProxyJump {spec:?} names no host")) };
        let (host, port, proxy) = resolve(first.host.clone(), first.port.unwrap_or(22), first.user.unwrap_or_default()).await;
        // As OpenSSH: the list's own port wins over the hop's config.
        let port = first.port.unwrap_or(port);
        match proxy {
            Some(Proxy::Jump(next)) => first_hop(&next, depth, resolve).await,
            Some(Proxy::Command(_)) => Ok(FirstHop::Command),
            _ => Ok(FirstHop::Address { label: first.host, host, port }),
        }
    })
}

/// `first_hop` with `~/.ssh/config` as the connect reads it.
pub(crate) async fn first_hop_in_config(spec: &str, depth: usize) -> Result<FirstHop, String> {
    let resolve = |host: String, port: u16, user: String| -> Pin<Box<dyn Future<Output = Resolved> + Send>> {
        Box::pin(async move {
            let mut t = crate::ConfigTarget::resolve(&host, port, &user, None).await;
            let proxy = t.proxy(None);
            (t.host, t.port, proxy)
        })
    };
    first_hop(spec, depth, &resolve).await
}

/// What `config_jump_chain` says when a route has more than
/// `MAX_JUMP_HOPS` hops (its own words).
fn too_many_hops() -> String {
    format!("ProxyJump in ~/.ssh/config: more than {} hops — a loop between its Host blocks?", crate::MAX_JUMP_HOPS)
}

/// How long a jump host's login may take, prompts included (login_hop).
pub(crate) const JUMP_LOGIN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// How long a jump host may take to open the channel to the next hop or the
/// server behind it (hop_channel), as for telnet through an SSH server.
pub(crate) const JUMP_CHANNEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// A `::sftp` / `::fwd` connection's jump host asked for a code (or a password
/// that isn't saved) that only the server's own tab can ask for.
pub(crate) const JUMP_ASKS_UNASKED: &str =
    "the jump host asks for a code or a password, and only the server's own tab can ask you for it";

/// `hop_channel`, given `limit` to open the channel in.
pub(crate) async fn hop_channel_within<H: russh::client::Handler>(
    via: &russh::client::Handle<H>,
    host: &str,
    port: u16,
    limit: std::time::Duration,
) -> Result<crate::tunnel::DrainedChannelStream, String> {
    let open = via.channel_open_direct_tcpip(host.to_string(), u32::from(port), "127.0.0.1", 0);
    // A shared login that can't open one takes no more tabs (#237): one the
    // jump host has no room on stays for the tabs on it; one that doesn't
    // answer, or has gone, is replaced by the next tab's login. A refusal
    // for the server behind it (it's down, or not allowed) is that server's,
    // not the login's.
    let failed = |full: bool| crate::jump_pool::POOL.channel_failed(via as *const russh::client::Handle<H> as usize, full);
    match tokio::time::timeout(limit, open).await {
        Ok(Ok(channel)) => Ok(crate::tunnel::drained_stream(channel)),
        Ok(Err(e)) => {
            match &e {
                russh::Error::ChannelOpenFailure(russh::ChannelOpenFailure::ResourceShortage) => failed(true),
                russh::Error::ChannelOpenFailure(_) => {}
                _ => failed(false),
            }
            Err(format!("could not open a channel to {host}:{port} through the jump host: {e}"))
        }
        Err(_) => {
            failed(false);
            Err(format!("the jump host didn't open a channel to {host}:{port} within {} seconds", limit.as_secs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_test_server::{client_over, connect, TestClient, TestServer};
    use russh::client;
    use russh::ChannelMsg;
    use std::time::Duration;

    /// Log in over `transport`, as the next hop of a route.
    async fn next_hop(transport: crate::tunnel::DrainedChannelStream) -> client::Handle<TestClient> {
        client_over(transport, client::Config::default(), TestClient::default()).await.expect("the next hop logs in")
    }

    /// Two hops, then a server behind them: a large download and upload at
    /// once go through, with no hop waiting on another for good.
    #[tokio::test]
    async fn a_two_hop_route_carries_traffic_both_ways_without_a_deadlock() {
        let hop1 = connect(TestServer::default(), |_| {}, client::Config::default()).await.unwrap();
        let hop2 = next_hop(crate::hop_channel(&hop1, "hop2", 22).await.unwrap()).await;
        let target = next_hop(crate::hop_channel(&hop2, "target", 22).await.unwrap()).await;

        const BYTES: usize = 6 * 1024 * 1024;
        let work = async {
            let mut flood = target.channel_open_session().await.unwrap();
            flood.exec(true, format!("flood {BYTES}")).await.unwrap();
            let mut echo = target.channel_open_session().await.unwrap();
            echo.exec(true, "echo").await.unwrap();
            let upload = vec![7u8; 2 * 1024 * 1024];
            let writer = echo.make_writer();
            let send = async move {
                let mut writer = writer;
                tokio::io::AsyncWriteExt::write_all(&mut writer, &upload).await.unwrap();
            };
            let read_flood = async {
                let mut got = 0;
                while let Some(msg) = flood.wait().await {
                    match msg {
                        ChannelMsg::Data { data } => got += data.len(),
                        ChannelMsg::Eof | ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                got
            };
            let read_echo = async {
                let mut got = 0;
                while got < 2 * 1024 * 1024 {
                    match echo.wait().await {
                        Some(ChannelMsg::Data { data }) => got += data.len(),
                        Some(_) => {}
                        None => break,
                    }
                }
                got
            };
            let ((), flooded, echoed) = tokio::join!(send, read_flood, read_echo);
            (flooded, echoed)
        };
        let (flooded, echoed) = tokio::time::timeout(Duration::from_secs(60), work).await.expect("no deadlock across the hops");
        assert_eq!(flooded, BYTES);
        assert_eq!(echoed, 2 * 1024 * 1024);
    }

    /// `~/.ssh/config` as a map: host → (HostName, Port, proxy line).
    fn config(entries: &[(&str, &str, u16, Option<Proxy>)]) -> impl Fn(String, u16, String) -> Pin<Box<dyn Future<Output = Resolved> + Send>> + Sync {
        let map: std::collections::HashMap<String, (String, u16, Option<Proxy>)> =
            entries.iter().map(|(a, h, p, x)| (a.to_string(), (h.to_string(), *p, x.clone()))).collect();
        move |host: String, port: u16, _user: String| {
            let found = map.get(&host).cloned().unwrap_or((host, port, None));
            Box::pin(async move { found })
        }
    }

    fn jump(spec: &str) -> Option<Proxy> {
        Some(Proxy::Jump(spec.into()))
    }

    #[test]
    fn a_bastion_has_one_queue_whatever_the_spelling() {
        assert_eq!(queue_key("Bastion.Example", 22), queue_key(" bastion.example", 22));
        assert_eq!(queue_key("::1", 22), queue_key("[::1]", 22));
        assert_ne!(queue_key("::1", 22), queue_key("::1:22", 22));
        assert_ne!(queue_key("bastion", 22), queue_key("bastion", 2222));
    }

    /// A hop dialled from here is known by its address, as a saved jump host
    /// and Monitoring know it; one reached through another hop, by that hop
    /// and its address, so the same private address behind two gateways
    /// gets two queues.
    #[test]
    fn a_hop_behind_another_is_queued_by_the_way_there() {
        let route_a = RouteQueues::default();
        assert_eq!(route_a.next(false, "gwA", 22), queue_key("gwa", 22));
        let behind_a = route_a.next(true, "10.0.0.5", 22);
        let route_b = RouteQueues::default();
        route_b.next(false, "gwB", 22);
        let behind_b = route_b.next(true, "10.0.0.5", 22);
        assert_ne!(behind_a, behind_b);
        assert_ne!(behind_a, queue_key("10.0.0.5", 22), "not the machine dialled directly at that address");
        // The same way there is the same queue, however deep.
        let again = RouteQueues::default();
        again.next(false, "GWA", 22);
        assert_eq!(again.next(true, "10.0.0.5", 22), behind_a);
        assert_eq!(again.next(true, "db", 2222), format!("{behind_a} > {}", queue_key("db", 2222)));
        // A saved jump host behind a chain: through its last hop.
        let saved = RouteQueues::default();
        saved.next(false, "gwA", 22);
        assert_eq!(saved.next(true, "10.0.0.5", 22), behind_a);
    }

    #[tokio::test]
    async fn the_first_hop_is_found_as_the_chain_would_dial_it() {
        let cfg = config(&[
            ("a", "a.example", 2200, None),
            ("b", "b.example", 22, jump("a")),
            ("c", "c.example", 22, Some(Proxy::Command("nc %h %p".into()))),
        ]);
        let addr = |label: &str, host: &str, port| FirstHop::Address { label: label.into(), host: host.into(), port };
        assert_eq!(first_hop("a,x", 0, &cfg).await.unwrap(), addr("a", "a.example", 2200));
        // The list's port wins over the config's.
        assert_eq!(first_hop("u@a:2222,x", 0, &cfg).await.unwrap(), addr("a", "a.example", 2222));
        // A first hop with its own ProxyJump: that one's first hop.
        assert_eq!(first_hop("b", 0, &cfg).await.unwrap(), addr("a", "a.example", 2200));
        assert_eq!(first_hop("c,b", 0, &cfg).await.unwrap(), FirstHop::Command);
        // A host the config doesn't know is dialled as written.
        assert_eq!(first_hop("[::1]:2022", 0, &cfg).await.unwrap(), addr("::1", "::1", 2022));
    }

    #[tokio::test]
    async fn a_proxy_jump_loop_ends_at_eight_hops() {
        let cfg = config(&[("a", "a", 22, jump("b")), ("b", "b", 22, jump("a"))]);
        let err = first_hop("a", 0, &cfg).await.unwrap_err();
        assert!(err.contains("more than 8 hops"), "{err}");
        // A long list counts too.
        let nine = "h1,h2,h3,h4,h5,h6,h7,h8,h9";
        assert!(first_hop(nine, 0, &config(&[])).await.unwrap_err().contains("more than 8 hops"));
        assert!(first_hop("h1,h2,h3,h4,h5,h6,h7,h8", 0, &config(&[])).await.is_ok());
        // A saved jump host's chain starts at depth 1 (the saved one is a hop).
        assert!(first_hop("h1,h2,h3,h4,h5,h6,h7", 1, &config(&[])).await.is_ok());
        assert!(first_hop("h1,h2,h3,h4,h5,h6,h7,h8", 1, &config(&[])).await.is_err());
    }

    /// The limit counts hops, not lists: a list whose first hop has a
    /// ProxyJump list of its own counts the hops of both.
    #[tokio::test]
    async fn hops_of_nested_lists_all_count_towards_eight() {
        let inner = |n: usize| (1..=n).map(|i| format!("g{i}")).collect::<Vec<_>>().join(",");
        let outer = |n: usize| (1..=n).map(|i| format!("h{i}")).collect::<Vec<_>>().join(",");
        for (o, i, ok) in [(4, 4, true), (8, 7, false), (4, 5, false), (1, 7, true), (2, 7, false)] {
            let cfg = config(&[("h1", "h1", 22, jump(&inner(i)))]);
            assert_eq!(first_hop(&outer(o), 0, &cfg).await.is_ok(), ok, "{o} hops, the first behind {i} more");
        }
    }

    #[tokio::test]
    async fn proxy_jump_tokens_and_bad_lists_are_refused_before_dialling() {
        for spec in ["%r@gate", "gate:%p", "%h", "a,b%%"] {
            let err = refuse_tokens(spec).unwrap_err();
            assert!(err.contains("% tokens"), "{err}");
            assert!(first_hop(spec, 0, &config(&[])).await.is_err());
        }
        assert!(refuse_tokens("alice@gate:2222,ssh://bob@inner").is_ok());
        // An IPv6 zone isn't a token: it gets its own message.
        for spec in ["[fe80::1%en0]", "u@[fe80::1%25eth0]:2222", "gate,fe80::1%en0"] {
            let err = refuse_tokens(spec).unwrap_err();
            assert!(err.contains("IPv6 address with a zone") && !err.contains("% tokens"), "{err}");
        }
        assert!(first_hop("u@", 0, &config(&[])).await.unwrap_err().contains("isn't a list"));
    }

    /// A hop that never answers the channel open ends with an error in time,
    /// instead of holding the attempt.
    #[tokio::test]
    async fn a_hop_that_never_opens_the_channel_times_out() {
        let hop = connect(TestServer::stalling_direct_tcpip(), |_| {}, client::Config::default())
            .await
            .unwrap();
        let started = std::time::Instant::now();
        let err = match super::hop_channel_within(&hop, "target", 22, Duration::from_millis(300)).await {
            Ok(_) => panic!("the channel can't open"),
            Err(e) => e,
        };
        assert!(err.contains("didn't open a channel to target:22"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    // Every hook into this file from the original project's code is where it
    // belongs (fork_hooks.rs): a merge that drops one fails here, while the
    // rule's own tests above would still pass.
    #[test]
    fn the_hooks_are_in_place() {
        use crate::fork_hooks::assert_hooks;
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("config_jump_chain", "hop_policy::refuse_tokens(", 1),
                ("config_jump_chain", "hop_policy::hop_turn(", 1),
                ("reach_hop", "hop_policy::hop_turn(", 3),
                ("hop_channel", "hop_policy::hop_channel_within(", 1),
                ("login_hop", "hop_policy::JUMP_LOGIN_DEADLINE", 2),
                ("login_hop", "hop_policy::JUMP_ASKS_UNASKED", 1),
                // A jump host login SSH terminal tabs share (#237): a hop
                // shares another tab's login or makes it for the pool, with
                // the shared window; the route keeps its leases.
                ("config_jump_chain", "hop_policy::HopTurn::Shared(h) => { chain.push(h); continue }", 1),
                ("reach_hop", "hop_policy::HopTurn::Shared(h) => { chain.push(h); return Ok(chain) }", 1),
                ("reach_hop", "hop_policy::HopTurn::Shared(h) => return Ok(vec![h])", 2),
                ("config_jump_chain", "let mut chain: Vec<hop_policy::HopHandle>", 1),
                ("", "Output = Result<Vec<hop_policy::HopHandle>, String>", 1),
                ("connect_jump_host", "-> Result<Vec<hop_policy::HopHandle>, String>", 1),
                ("login_hop", "-> Result<hop_policy::HopHandle, String>", 1),
                ("login_hop", "let config = ctx.queues.tune(client_config_for_target(", 1),
                ("login_hop", "Ok(ctx.queues.keep(session))", 1),
                ("initiate_connection", "queues: hop_policy::RouteQueues::shared(&app, &session_id_clone)", 1),
                ("initiate_connection", "let mut jump_handles: Vec<hop_policy::HopHandle>", 1),
                ("initiate_connection", "let chain: Option<Result<Vec<hop_policy::HopHandle>, String>>", 1),
                ("initiate_connection", "let handles = std::mem::take(&mut jump_handles);", 1),
                ("initiate_connection", "leases: hop_ctx.queues.take_leases()", 1),
                // RDP, telnet, FTP and VNC through a jump host keep logins of
                // their own.
                ("initiate_rdp", "connect_jump_host(&ctx, jid).await.and_then(hop_policy::owned)", 1),
                ("rdp_builtin_open", "connect_jump_host(&ctx, jid).await.and_then(hop_policy::owned)", 1),
                ("telnet_connect", "connect_jump_host(&ctx, jid).await.and_then(hop_policy::owned)", 1),
                ("ftp_connect", "connect_jump_host(&ctx, jid).await.and_then(hop_policy::owned)", 1),
                ("vnc_open", "connect_jump_host(&ctx, jid).await.and_then(hop_policy::owned)", 1),
                ("", "mod jump_pool;", 1),
                ("", "jump_pool::get_jump_sharing, jump_pool::set_jump_sharing,", 1),
            ],
        );
        // The pool is emptied with the profile, and a deleted jump host's
        // logins take no more tabs (fork files, but the pool depends on them).
        assert_hooks(
            "ssh_manager/root_state.rs",
            include_str!("ssh_manager/root_state.rs"),
            &[("forget_fork_sessions", "crate::jump_pool::clear()", 1), ("", "pub leases: crate::jump_pool::Leases,", 1)],
        );
        assert_hooks(
            "jump_gone.rs",
            include_str!("jump_gone.rs"),
            &[("note_behind", "crate::jump_pool::forget_saved(", 1), ("note_lost", "crate::jump_pool::forget_saved(", 1)],
        );
        assert_hooks("monitor.rs", include_str!("monitor.rs"), &[("run_poller", "hop_policy::queue_key(", 1)]);
        // Moved out of lib.rs into a fork file of its own (#136).
        assert_hooks("connect_diag.rs", include_str!("connect_diag.rs"), &[("first_hop_route", "hop_policy::first_hop_in_config(", 1)]);
    }
}

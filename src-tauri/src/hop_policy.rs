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
pub(crate) fn refuse_tokens(spec: &str) -> Result<(), String> {
    if spec.contains('%') {
        return Err(format!(
            "ProxyJump {spec:?} in ~/.ssh/config uses % tokens (such as %h or %r), which Submarine doesn't fill in yet. \
             Write the jump hosts out in full."
        ));
    }
    Ok(())
}

/// The login queue a jump host waits in (multiexec::login_turn): its address
/// as dialled, so a saved jump host and the same bastion in a ProxyJump chain
/// share one queue (#43, D1).
pub(crate) fn queue_key(host: &str, port: u16) -> String {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    format!("[{host}]:{port}")
}

/// A turn to log in to `target`, a hop of `ctx`'s route, from the moment its
/// transport opens until its login is over (drop it then). One hop's turn is
/// never held while the next hop waits for one: two routes through the same
/// bastions in opposite orders would wait on each other for good.
pub(crate) async fn hop_turn(
    ctx: &crate::HopCtx<'_>,
    target: &crate::ConfigTarget,
) -> Result<Option<tokio::sync::OwnedSemaphorePermit>, String> {
    let key = queue_key(&target.host, target.port);
    crate::multiexec::login_turn(ctx.app, &key, ctx.attempt, |limit| {
        ctx.log(
            &format!("Waiting for a login slot on the jump host {} ({limit} logging in at a time)...", target.host),
            "info",
        )
    })
    .await
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
        let err = match crate::hop_channel_within(&hop, "target", 22, Duration::from_millis(300)).await {
            Ok(_) => panic!("the channel can't open"),
            Err(e) => e,
        };
        assert!(err.contains("didn't open a channel to target:22"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}

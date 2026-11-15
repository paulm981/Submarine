//! The pool's tests: which tabs share a login and which don't, its cap, its
//! linger, a dropped login, a tab closed while it makes one, a refused
//! login, and the connections that never share. Logins are real russh logins
//! to the in-process test server, counted where they're dialled
//! (`ShapedLink::dials`). The speed of a shared login is tested in
//! ssh_manager/fork_tests/jump_share.rs, through the terminal's own pump.

use super::shaped_link::{LinkSpec, Queuing, ShapedLink};
use super::*;
use crate::ssh_manager::ConnectAttempt;
use crate::ssh_test_server::{client_over, serve, TestClient, TestServer};
use russh::client;
use std::collections::HashMap;

type Gens = Arc<tokio::sync::Mutex<HashMap<String, u64>>>;

/// "Logins per jump host", as it is until changed (multiexec.rs).
const LIMIT: usize = crate::multiexec::DEFAULT_LOGIN_LIMIT as usize;

/// A tab's connect attempt.
struct Tab {
    id: String,
    gens: Gens,
    attempt: ConnectAttempt,
}

fn tab(id: &str) -> Tab {
    let gens: Gens = Arc::new(tokio::sync::Mutex::new(HashMap::from([(id.to_string(), 1)])));
    let attempt = ConnectAttempt::new(Arc::clone(&gens), id.to_string(), 1);
    Tab { id: id.to_string(), gens, attempt }
}

impl Tab {
    /// The tab is closed (or reconnects): its attempt is no longer current.
    async fn close(&self) {
        *self.gens.lock().await.get_mut(&self.id).unwrap() += 1;
    }
}

fn key(user: &str, via: Via, login: &str, secret: &[u8]) -> Key {
    Key::new(via, crate::hop_policy::queue_key("bastion", 22), user.into(), login.into(), digest(&[secret]))
}

fn alice() -> Key {
    key("alice", Via::Direct, "jump1", b"password")
}

fn link() -> Arc<ShapedLink> {
    ShapedLink::new(LinkSpec::new(100, Duration::from_millis(20), 4 << 20, Queuing::Fair))
}

/// A login to the jump host over `link`, with the shared window.
async fn log_in(link: &ShapedLink, server: TestServer) -> Result<client::Handle<TestClient>, russh::Error> {
    let (c, s) = link.dial();
    serve(server, |_| {}, s);
    client_over(c, tune(client::Config::default()), TestClient::default()).await
}

/// A login the jump host refuses (it takes no key, and this one offers a key).
async fn refused_login(link: &ShapedLink) -> Result<client::Handle<TestClient>, russh::Error> {
    let (c, s) = link.dial();
    serve(
        TestServer::accepting_key(None, false),
        |cfg| {
            cfg.auth_rejection_time = Duration::ZERO;
            cfg.auth_rejection_time_initial = Some(Duration::ZERO);
        },
        s,
    );
    let mut h = client::connect_stream(Arc::new(tune(client::Config::default())), c, TestClient::default()).await?;
    let k = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    if crate::ssh_manager::authenticate_with_key(&mut h, "alice", k).await? {
        Ok(h)
    } else {
        Err(russh::Error::NotAuthenticated)
    }
}

/// What a tab does for one hop (hop_policy::hop_turn and lib.rs login_hop):
/// shares the login for `key`, or makes it, given up on if the tab closes
/// meanwhile; a login that fails tells the waiting tabs how
/// (hop_policy::outcome_of).
async fn open<F, Fut>(pool: &Arc<Pool<TestClient>>, key: &Key, tab: &Tab, login: F) -> Result<(Shared<TestClient>, Lease<TestClient>), Waited>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<client::Handle<TestClient>, russh::Error>>,
{
    open_capped(pool, key, tab, LIMIT, login).await
}

/// `open`, with "Logins per jump host" at `max`.
async fn open_capped<F, Fut>(pool: &Arc<Pool<TestClient>>, key: &Key, tab: &Tab, max: usize, login: F) -> Result<(Shared<TestClient>, Lease<TestClient>), Waited>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<client::Handle<TestClient>, russh::Error>>,
{
    match pool.acquire(key.clone(), &tab.id, &tab.attempt, max, || {}).await? {
        Acquired::Shared(h, lease) => Ok((h, lease)),
        Acquired::Create(mut reservation) => {
            let made = tokio::select! {
                r = login() => r,
                _ = tab.attempt.superseded() => {
                    reservation.fail(crate::hop_policy::outcome_of(tab.attempt.is_current().await, false, false));
                    return Err(Waited::Cancelled);
                }
            };
            match made {
                Ok(h) => {
                    let h = Arc::new(h);
                    let lease = reservation.ready(Arc::clone(&h), None, None).expect("the pool keeps it");
                    Ok((h, lease))
                }
                Err(e) => {
                    let auth = matches!(e, russh::Error::NotAuthenticated);
                    let outcome = crate::hop_policy::outcome_of(tab.attempt.is_current().await, auth, !auth);
                    reservation.fail(outcome);
                    let Outcome::Failed { auth, retry } = outcome else { panic!("{outcome:?}") };
                    Err(Waited::Failed { auth, retry })
                }
            }
        }
    }
}

async fn open_on(pool: &Arc<Pool<TestClient>>, key: &Key, tab: &Tab, link: &ShapedLink) -> (Shared<TestClient>, Lease<TestClient>) {
    open_on_capped(pool, key, tab, link, LIMIT).await
}

async fn open_on_capped(pool: &Arc<Pool<TestClient>>, key: &Key, tab: &Tab, link: &ShapedLink, max: usize) -> (Shared<TestClient>, Lease<TestClient>) {
    open_capped(pool, key, tab, max, || log_in(link, TestServer::default())).await.expect("a login")
}

/// A server behind the jump host, through a channel on `login`, as a tab's
/// own session there.
async fn target(login: &client::Handle<TestClient>) -> client::Handle<TestClient> {
    let stream = crate::hop_channel(login, "target", 22).await.expect("a channel through the jump host");
    client_over(stream, client::Config::default(), TestClient::default()).await.expect("the server behind it logs in")
}

#[tokio::test(start_paused = true)]
async fn tabs_with_the_same_login_share_one() {
    let (pool, link) = (Pool::new(), link());
    let (a, b) = (tab("a"), tab("b"));
    let (ha, _la) = open_on(&pool, &alice(), &a, &link).await;
    let (hb, _lb) = open_on(&pool, &alice(), &b, &link).await;
    assert!(Arc::ptr_eq(&ha, &hb));
    assert_eq!(link.dials(), 1, "one login to the jump host");
    assert_eq!(pool.census(&alice()), vec![2]);
    // Each tab reaches its own server through it.
    let (ta, tb) = (target(&ha).await, target(&hb).await);
    assert!(!ta.is_closed() && !tb.is_closed());
}

/// The security rule (#237): a login is shared only with a tab that would
/// log in the same way: the same user, credentials, saved jump host, way
/// there and profile.
#[tokio::test(start_paused = true)]
async fn a_different_user_key_saved_login_route_or_profile_never_shares() {
    let (pool, link) = (Pool::new(), link());
    let first = tab("first");
    let (h, _l) = open_on(&pool, &alice(), &first, &link).await;
    let others = [
        ("a different user", key("bob", Via::Direct, "jump1", b"password")),
        ("a different password or key", key("alice", Via::Direct, "jump1", b"another key")),
        ("a different saved jump host at the same address", key("alice", Via::Direct, "jump2", b"password")),
        ("the same host:port through a ProxyCommand", key("alice", Via::Command("nc gw 22".into()), "jump1", b"password")),
        ("the same host:port behind another jump host", key("alice", Via::Through(9_999), "jump1", b"password")),
    ];
    let mut kept = Vec::new();
    for (n, (what, k)) in others.iter().enumerate() {
        let t = tab(&format!("other{n}"));
        let (other, lease) = open_on(&pool, k, &t, &link).await;
        assert!(!Arc::ptr_eq(&h, &other), "{what} shared the first tab's login");
        assert_eq!(link.dials(), n + 2, "{what} made its own login");
        kept.push((other, lease));
    }
    // A different profile: the pool was emptied when the first one closed,
    // and its logins are never used again.
    pool.clear();
    let next = tab("next profile");
    let (other, _l2) = open_on(&pool, &alice(), &next, &link).await;
    assert!(!Arc::ptr_eq(&h, &other), "the next profile shared the last one's login");
    assert_eq!(link.dials(), others.len() + 2);
}

/// Who logs in, where and how is in the key. Two hops that differ in any of
/// it get different keys.
#[tokio::test]
async fn the_key_holds_who_logs_in_where_and_how() {
    use crate::hop_policy::pool_key;
    let hop = |user: &str, password: Option<&str>, key: Option<&str>, cache_id: &str| crate::Hop {
        target: crate::ConfigTarget::apply("bastion", 22, user, None),
        password: password.map(str::to_string),
        key_data: key.map(|k| (k.to_string(), None, None)),
        cache_id: cache_id.into(),
        save_to: None,
    };
    let base = || hop("alice", Some("pw"), None, "jump1");
    let k = |h: &crate::Hop, via: Via, agent: bool| pool_key(h, Some(via), agent).expect("shared");
    let same = k(&base(), Via::Direct, true);
    assert!(same == k(&base(), Via::Direct, true), "the same login, the same key");
    for (what, other) in [
        ("user", k(&hop("bob", Some("pw"), None, "jump1"), Via::Direct, true)),
        ("password", k(&hop("alice", Some("pw2"), None, "jump1"), Via::Direct, true)),
        ("no password", k(&hop("alice", None, None, "jump1"), Via::Direct, true)),
        ("key", k(&hop("alice", Some("pw"), Some("KEY A"), "jump1"), Via::Direct, true)),
        ("saved jump host", k(&hop("alice", Some("pw"), None, "jump2"), Via::Direct, true)),
        ("route", k(&base(), Via::Command("nc %h %p".into()), true)),
        ("hop before", k(&base(), Via::Through(7), true)),
        ("agent setting", k(&base(), Via::Direct, false)),
    ] {
        assert!(other != same, "a different {what} gave the same key");
    }
    assert!(k(&hop("alice", Some("pw"), Some("KEY A"), "jump1"), Via::Direct, true) != k(&hop("alice", Some("pw"), Some("KEY B"), "jump1"), Via::Direct, true));
    // A user asked for at login isn't known yet: not shared.
    assert!(pool_key(&hop("", Some("pw"), None, "jump1"), Some(Via::Direct), true).is_none());
    assert!(pool_key(&hop("  ", Some("pw"), None, "jump1"), Some(Via::Direct), true).is_none());
    // Nor a hop behind one that isn't in the pool.
    assert!(pool_key(&base(), None, true).is_none());
    // The secrets stay out of what the key prints.
    assert!(!format!("{same:?}").contains("pw"));
}

/// Opens `n` tabs one after another; the tabs, their logins and leases.
async fn tabs_on(pool: &Arc<Pool<TestClient>>, link: &ShapedLink, n: usize, max: usize) -> Vec<(Tab, Shared<TestClient>, Lease<TestClient>)> {
    let mut tabs = Vec::new();
    for i in 0..n {
        let t = tab(&format!("t{i}"));
        let (h, lease) = open_on_capped(pool, &alice(), &t, link, max).await;
        tabs.push((t, h, lease));
    }
    tabs
}

/// Like a thread pool: one login for one or two tabs, a second at the third
/// tab, and so on, one more login for every two tabs, up to "Logins per jump
/// host" (5 here: 5 logins from the ninth tab on).
#[tokio::test(start_paused = true)]
async fn a_key_grows_a_login_for_every_two_tabs_up_to_the_setting() {
    let (pool, link) = (Pool::new(), link());
    let mut tabs = Vec::new();
    for n in 1..=14usize {
        let t = tab(&format!("t{n}"));
        tabs.push((open_on_capped(&pool, &alice(), &t, &link, 5).await, t));
        let logins = n.div_ceil(2).min(5);
        assert_eq!(link.dials(), logins, "{n} tabs");
        assert_eq!(pool.census(&alice()).len(), logins, "{n} tabs");
    }
    let mut counts = pool.census(&alice());
    counts.sort_unstable();
    assert_eq!(counts, vec![2, 3, 3, 3, 3]);
}

/// Never more logins for one key than the setting says, however many tabs
/// open at once: 3 here, so 30 tabs at once are 3 logins of 10, made one
/// after another.
#[tokio::test(start_paused = true)]
async fn a_key_never_has_more_logins_than_the_setting() {
    let (pool, link) = (Pool::new(), link());
    let tabs: Vec<Tab> = (0..30).map(|n| tab(&format!("t{n}"))).collect();
    let k = alice();
    let opened = futures::future::join_all(tabs.iter().map(|t| open_on_capped(&pool, &k, t, &link, 3))).await;
    assert_eq!(link.dials(), 3);
    assert_eq!(pool.census(&alice()), vec![10, 10, 10]);
    // More come later: still 3.
    let more = tabs_on(&pool, &link, 6, 3).await;
    assert_eq!(link.dials(), 3);
    assert_eq!(pool.census(&alice()), vec![12, 12, 12]);
    drop((opened, more));
}

/// 0 means no cap: a login for every two tabs.
#[tokio::test(start_paused = true)]
async fn with_no_cap_a_key_has_a_login_for_every_two_tabs() {
    let (pool, link) = (Pool::new(), link());
    let tabs = tabs_on(&pool, &link, 23, 0).await;
    assert_eq!(link.dials(), 12);
    assert_eq!(pool.census(&alice()).iter().sum::<usize>(), 23);
    assert!(pool.census(&alice()).iter().all(|&n| n <= 2));
    drop(tabs);
}

/// The default, 8: a folder of 40 servers opened at once is 8 logins of 5.
#[tokio::test(start_paused = true)]
async fn many_tabs_at_once_spread_over_the_settings_logins() {
    let (pool, link) = (Pool::new(), link());
    let tabs: Vec<Tab> = (0..40).map(|n| tab(&format!("t{n}"))).collect();
    let k = alice();
    let opened = futures::future::join_all(tabs.iter().map(|t| open_on(&pool, &k, t, &link))).await;
    assert_eq!(link.dials(), LIMIT);
    assert_eq!(pool.census(&alice()), vec![5; LIMIT]);
    drop(opened);
}

/// A new tab goes on the least busy login (the oldest of those as busy).
#[tokio::test(start_paused = true)]
async fn a_tab_goes_on_the_least_busy_login() {
    let (pool, link) = (Pool::new(), link());
    let mut tabs = tabs_on(&pool, &link, 10, 5).await;
    // Logins A..E with two tabs each, in that order. Both of C's tabs close,
    // and one of A's.
    let c = Arc::clone(&tabs[4].1);
    let a = Arc::clone(&tabs[0].1);
    assert!(Arc::ptr_eq(&c, &tabs[5].1) && !Arc::ptr_eq(&a, &c));
    tabs.remove(5);
    tabs.remove(4);
    tabs.remove(0);
    let t = tab("next");
    let (h, _l1) = open_on_capped(&pool, &alice(), &t, &link, 5).await;
    assert!(Arc::ptr_eq(&h, &c), "C has none");
    let t2 = tab("after");
    let (h2, _l2) = open_on_capped(&pool, &alice(), &t2, &link, 5).await;
    assert!(Arc::ptr_eq(&h2, &a), "A and C have one each: A is older");
    assert_eq!(link.dials(), 5);
}

/// The second login of a key gets the password typed for the first (copied
/// into its tab's slot by hop_policy::hop_turn), so its tab isn't asked again.
#[tokio::test(start_paused = true)]
async fn a_second_login_gets_the_password_typed_for_the_first() {
    let (pool, link) = (Pool::new(), link());
    let first = tab("first");
    let Acquired::Create(r) = pool.acquire(alice(), "first|jump1|bastion:22|alice", &first.attempt, LIMIT, || {}).await.unwrap() else { panic!("creates") };
    assert!(r.typed.is_none() && r.typed_at.is_none(), "nothing typed before the first login");
    let h = Arc::new(log_in(&link, TestServer::default()).await.unwrap());
    let mut leases = vec![r.ready(Arc::clone(&h), None, Some(zeroize::Zeroizing::new("typed".into()))).unwrap()];
    let t = tab("t1");
    let Acquired::Shared(_, l) = pool.acquire(alice(), &t.id, &t.attempt, LIMIT, || {}).await.unwrap() else { panic!("shares") };
    leases.push(l);
    let late = tab("late");
    let Acquired::Create(second) = pool.acquire(alice(), "late|jump1|bastion:22|alice", &late.attempt, LIMIT, || {}).await.unwrap() else { panic!("a second login") };
    assert_eq!(second.typed.as_deref().map(String::as_str), Some("typed"));
    assert_eq!(second.typed_at.as_deref(), Some("first|jump1|bastion:22|alice"));
    // Another key never gets it.
    let b = tab("b");
    let Acquired::Create(other) = pool.acquire(key("bob", Via::Direct, "jump1", b"password"), "b", &b.attempt, LIMIT, || {}).await.unwrap() else { panic!("creates") };
    assert!(other.typed.is_none() && other.typed_at.is_none());
}

#[tokio::test(start_paused = true)]
async fn a_login_no_tab_uses_closes_thirty_seconds_after_the_last_one() {
    let (pool, link) = (Pool::new(), link());
    let a = tab("a");
    let (h, lease) = open_on(&pool, &alice(), &a, &link).await;
    drop(lease);
    // A tab back within the 30 seconds (a reconnect) uses it again.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let b = tab("b");
    let (hb, lease) = open_on(&pool, &alice(), &b, &link).await;
    assert!(Arc::ptr_eq(&h, &hb));
    assert_eq!(link.dials(), 1);
    // The count starts again from the last tab gone.
    drop(lease);
    tokio::time::sleep(Duration::from_secs(29)).await;
    assert_eq!(pool.census(&alice()), vec![0], "still open at 29 seconds");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(pool.census(&alice()).is_empty(), "closed at 31 seconds");
    // With no tab holding it either, the next tab logs in afresh.
    drop((h, hb));
    let c = tab("c");
    let _new = open_on(&pool, &alice(), &c, &link).await;
    assert_eq!(link.dials(), 2);
}

/// Waits until `h` has seen its connection go.
async fn until_closed(h: &client::Handle<TestClient>) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !h.is_closed() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the login drops");
}

/// A login drops: its tabs reconnect onto the key's other logins, and only
/// when none is left is a new one made.
#[tokio::test(start_paused = true)]
async fn tabs_on_a_dropped_login_reconnect_onto_the_others() {
    let (pool, link) = (Pool::new(), link());
    // A has two tabs, B one; then one of A's closes: one each.
    let mut tabs = tabs_on(&pool, &link, 3, 5).await;
    let (a, b) = (Arc::clone(&tabs[0].1), Arc::clone(&tabs[2].1));
    assert!(!Arc::ptr_eq(&a, &b));
    tabs.remove(1);
    link.cut_one(0);
    until_closed(&a).await;
    // A's tab reconnects (letting go of its old route first): onto B.
    tabs.remove(0);
    let t = tab("t0 again");
    let (h, lease) = open_on_capped(&pool, &alice(), &t, &link, 5).await;
    assert!(Arc::ptr_eq(&h, &b));
    assert_eq!(link.dials(), 2, "no new login");
    // B drops too: nothing left, so the tabs make a new one between them.
    link.cut_one(1);
    until_closed(&b).await;
    drop((tabs, h, lease));
    let back: Vec<Tab> = (0..2).map(|n| tab(&format!("back{n}"))).collect();
    let k = alice();
    let opened = futures::future::join_all(back.iter().map(|t| open_on_capped(&pool, &k, t, &link, 5))).await;
    assert_eq!(link.dials(), 3);
    assert!(Arc::ptr_eq(&opened[0].0, &opened[1].0) && !opened[0].0.is_closed());
}

/// Every login of a key drops: every tab sees its session end, and their
/// reconnects make the logins again, one after another, not one each.
#[tokio::test(start_paused = true)]
async fn dropped_logins_are_made_again_once_for_all_their_tabs() {
    let (pool, link) = (Pool::new(), link());
    let tabs: Vec<Tab> = (0..10).map(|n| tab(&format!("t{n}"))).collect();
    let mut sessions = Vec::new();
    for t in &tabs {
        let (h, lease) = open_on_capped(&pool, &alice(), t, &link, 5).await;
        let server = target(&h).await;
        sessions.push((h, lease, server));
    }
    assert_eq!(link.dials(), 5);
    link.cut();
    // Every tab's session ends (its watcher would say so and reconnect).
    tokio::time::timeout(Duration::from_secs(10), async {
        while !sessions.iter().all(|(_, _, s)| s.is_closed()) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("every tab sees the logins drop");
    // A reconnect lets go of the tab's old route first (initiate_connection).
    drop(sessions);
    let reconnects: Vec<Tab> = (0..10).map(|n| tab(&format!("t{n} again"))).collect();
    let k = alice();
    let back = futures::future::join_all(reconnects.iter().map(|t| async {
        let (h, lease) = open_on_capped(&pool, &k, t, &link, 5).await;
        let server = target(&h).await;
        (h, lease, server)
    }))
    .await;
    assert_eq!(link.dials(), 10, "five new logins for the ten tabs");
    assert_eq!(pool.census(&alice()), vec![2; 5]);
    assert!(back.iter().all(|(_, _, s)| !s.is_closed()));
}

/// The shared login drops while its tabs still hold it: the next tab finds
/// it closed and makes a new one, though no tab has let go of the old.
#[tokio::test(start_paused = true)]
async fn a_dropped_login_is_replaced_even_while_tabs_still_hold_it() {
    let (pool, link) = (Pool::new(), link());
    let a = tab("a");
    let (h, _held) = open_on(&pool, &alice(), &a, &link).await;
    link.cut();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !h.is_closed() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let b = tab("b");
    let (hb, _l) = open_on(&pool, &alice(), &b, &link).await;
    assert!(!hb.is_closed());
    assert_eq!(link.dials(), 2);
}

/// The tab making the login is closed before it's done: a tab waiting on it
/// makes it instead (its prompts move there).
#[tokio::test(start_paused = true)]
async fn a_waiting_tab_takes_over_when_the_first_is_closed() {
    let pool = Pool::new();
    // A slow jump host: its login takes seconds.
    let link = ShapedLink::new(LinkSpec::new(100, Duration::from_millis(800), 4 << 20, Queuing::Fair));
    let (first, second) = (tab("first"), tab("second"));
    let k = alice();
    let making = open(&pool, &k, &first, || log_in(&link, TestServer::default()));
    let waiting = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        open_on(&pool, &alice(), &second, &link).await
    };
    let closing = async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        first.close().await;
    };
    let (made, (h, _lease), ()) = tokio::join!(making, waiting, closing);
    assert_eq!(made.err(), Some(Waited::Cancelled));
    assert!(!h.is_closed());
    assert_eq!(link.dials(), 2, "the second tab made its own login once the first was closed");
    assert_eq!(pool.census(&alice()), vec![1]);
}

/// The jump host refuses the login: every tab waiting on it fails too, as an
/// auth error (so none of them reconnects to ask again), with one login
/// tried, not one each.
#[tokio::test(start_paused = true)]
async fn a_refused_login_fails_every_waiting_tab_as_an_auth_error() {
    let (pool, link) = (Pool::new(), link());
    let creator = tab("creator");
    let waiters: Vec<Tab> = (0..3).map(|n| tab(&format!("w{n}"))).collect();
    let k = alice();
    let making = open(&pool, &k, &creator, || refused_login(&link));
    let waiting = futures::future::join_all(waiters.iter().map(|t| async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        open(&pool, &k, t, || async { panic!("a waiting tab logged in itself") }).await
    }));
    let (made, waited) = tokio::join!(making, waiting);
    assert_eq!(made.err(), Some(Waited::Failed { auth: true, retry: false }));
    for w in waited {
        assert_eq!(w.err(), Some(Waited::Failed { auth: true, retry: false }));
    }
    assert_eq!(link.dials(), 1);
    assert_eq!(pool.logins(), 0, "nothing left behind");
}

#[test]
fn how_an_unfinished_login_ends_for_the_tabs_waiting_on_it() {
    use crate::hop_policy::outcome_of;
    // Its tab closed (even mid-prompt, which cancels the prompt): another tab
    // makes it.
    assert_eq!(outcome_of(false, true, false), Outcome::Abandoned);
    assert_eq!(outcome_of(true, true, false), Outcome::Failed { auth: true, retry: false });
    assert_eq!(outcome_of(true, true, true), Outcome::Failed { auth: true, retry: false });
    assert_eq!(outcome_of(true, false, true), Outcome::Failed { auth: false, retry: true });
    assert_eq!(outcome_of(true, false, false), Outcome::Abandoned);
}

/// A tab's `::sftp` and `::fwd` connections, a terminal's own login, and
/// every connection while the setting is off, keep logins of their own.
#[test]
fn only_an_ssh_tabs_own_connection_shares_and_only_when_on() {
    use crate::hop_policy::RouteQueues;
    assert!(RouteQueues::sharing(true, "session-12").shares());
    for secondary in ["session-12::sftp", "session-12::fwd", "session-12::term-3"] {
        assert!(!RouteQueues::sharing(true, secondary).shares(), "{secondary}");
    }
    assert!(!RouteQueues::sharing(false, "session-12").shares());
    // RDP, VNC, telnet and FTP build their route with `Default`.
    assert!(!RouteQueues::default().shares());
}

/// A login that stopped answering (a channel open timed out, no room on it,
/// or gone) takes no more tabs; its own tabs carry on until they reconnect.
#[tokio::test(start_paused = true)]
async fn an_unhealthy_login_takes_no_more_tabs() {
    let (pool, link) = (Pool::new(), link());
    let tabs = tabs_on(&pool, &link, 3, 5).await;
    let (a, b) = (Arc::clone(&tabs[0].1), Arc::clone(&tabs[2].1));
    // B has one tab, so it's the least busy: it's B that goes.
    pool.unhealthy(Arc::as_ptr(&b) as usize);
    assert_eq!(pool.census(&alice()), vec![2]);
    assert!(!target(&b).await.is_closed(), "its tab carries on");
    // A has two: the next tab makes a new login rather than go on B.
    let t = tab("next");
    let (h, _l) = open_on_capped(&pool, &alice(), &t, &link, 5).await;
    assert!(!Arc::ptr_eq(&h, &b) && !Arc::ptr_eq(&h, &a));
    assert_eq!(link.dials(), 3);
    // And A, unhealthy too: tabs go on the new login only.
    pool.unhealthy(Arc::as_ptr(&a) as usize);
    let t2 = tab("after");
    let (h2, _l2) = open_on_capped(&pool, &alice(), &t2, &link, 5).await;
    assert!(Arc::ptr_eq(&h2, &h));
    drop(tabs);
}

/// A deleted jump host's logins take no more tabs; a tab on one carries on.
#[tokio::test(start_paused = true)]
async fn a_deleted_jump_hosts_login_takes_no_more_tabs() {
    let (pool, link) = (Pool::new(), link());
    let a = tab("a");
    let (h, _l) = open_on(&pool, &alice(), &a, &link).await;
    let other = key("alice", Via::Direct, "jump2", b"password");
    let o = tab("o");
    let _other = open_on(&pool, &other, &o, &link).await;
    pool.forget_login("jump1");
    assert!(pool.census(&alice()).is_empty());
    assert_eq!(pool.census(&other), vec![1], "another jump host's login stays");
    assert!(!target(&h).await.is_closed(), "the tab on it carries on");
    let b = tab("b");
    let (hb, _lb) = open_on(&pool, &alice(), &b, &link).await;
    assert!(!Arc::ptr_eq(&h, &hb));
}

/// A login still being made when the profile closes isn't kept for the next
/// profile: the tab making it keeps it to itself.
#[tokio::test(start_paused = true)]
async fn a_login_made_across_a_profile_close_isnt_shared() {
    let (pool, link) = (Pool::new(), link());
    let a = tab("a");
    let Acquired::Create(reservation) = pool.acquire(alice(), "a", &a.attempt, LIMIT, || {}).await.unwrap() else { panic!("creates") };
    let h = Arc::new(log_in(&link, TestServer::default()).await.unwrap());
    pool.clear();
    assert!(reservation.ready(Arc::clone(&h), None, None).is_none());
    let b = tab("b");
    let (hb, _l) = open_on(&pool, &alice(), &b, &link).await;
    assert!(!Arc::ptr_eq(&h, &hb));
}

/// A tab waiting on another's login stops when it is closed itself.
#[tokio::test(start_paused = true)]
async fn a_waiting_tab_that_is_closed_stops_waiting() {
    let pool: Arc<Pool<TestClient>> = Pool::new();
    let a = tab("a");
    let _making = pool.acquire(alice(), "a", &a.attempt, LIMIT, || {}).await.unwrap();
    let b = tab("b");
    let waited = async { pool.acquire(alice(), "b", &b.attempt, LIMIT, || {}).await };
    let closing = async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        b.close().await;
    };
    let (waited, ()) = tokio::join!(waited, closing);
    assert_eq!(waited.err(), Some(Waited::Cancelled));
}

/// The hop behind a shared login holds it: the first hop lingers as long as
/// the second does.
#[tokio::test(start_paused = true)]
async fn a_hop_behind_a_shared_login_holds_it() {
    let (pool, link) = (Pool::new(), link());
    let a = tab("a");
    let (h1, l1) = open_on(&pool, &alice(), &a, &link).await;
    let second = key("alice", Via::Through(l1.login_id()), "cfgjump|inner", b"");
    let Acquired::Create(reservation) = pool.acquire(second.clone(), "a", &a.attempt, LIMIT, || {}).await.unwrap() else { panic!("creates") };
    let h2 = Arc::new(client_over(crate::hop_channel(&h1, "inner", 22).await.unwrap(), tune(client::Config::default()), TestClient::default()).await.unwrap());
    let l2 = reservation.ready(Arc::clone(&h2), pool.lease_again(&l1), None).unwrap();
    assert_eq!(pool.census(&alice()), vec![2]);
    drop((l1, l2));
    tokio::time::sleep(Duration::from_secs(31)).await;
    // The second went at 30 s, and let go of the first, which goes 30 s on.
    assert!(pool.census(&second).is_empty());
    assert_eq!(pool.census(&alice()), vec![0]);
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert!(pool.census(&alice()).is_empty());
}

#[test]
fn routes_of_protocols_that_keep_their_own_logins_unwrap() {
    // Nothing shares their handles, so `owned` hands them back.
    assert!(crate::hop_policy::owned(Vec::new()).unwrap().is_empty());
}

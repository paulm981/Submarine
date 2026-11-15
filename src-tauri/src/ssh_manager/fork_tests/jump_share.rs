//! The speed of a jump host login that SSH terminal tabs share (#237),
//! against each tab logging in itself, through the terminal's own pump
//! (`pty_pump`). The jump host is the in-process test server, reached over a
//! `ShapedLink` (a delay, a rate and a bottleneck's buffer, FIFO or fair) on a
//! paused clock: what's measured is queuing and delay, the part sharing
//! changes, not the CPU time of the machine running the tests, so the numbers
//! are the same on every run.
//!
//! What a shared login may cost, and what's checked here:
//! - Nothing for a tab's echo when the link has room: the same hops, the same
//!   encryption (`echo_through_a_shared_login_is_as_fast_as_through_its_own`).
//! - While another tab on it floods a saturated link, a tab's echo waits
//!   behind at most the shared window (`jump_pool::WINDOW`) of the flood: on a
//!   FIFO link (a home router, a VPN) less than behind a flood on a login of
//!   its own, whose 8 MiB window fills the queue; on a fair one (fq_codel) up
//!   to 512 KiB at the link's speed more (42 ms at 100 Mbit/s). The flooding
//!   tab keeps 80% of what the link or the window allows
//!   (`an_idle_tab_echoes_at_speed_while_its_neighbour_floods`).
//! - A tab that stops reading holds up no other tab on the login, and is cut
//!   alone (`a_stalled_tab_never_holds_up_the_shared_connection`).

use super::super::*;
use crate::jump_pool::shaped_link::{LinkSpec, Queuing, ShapedLink};
use crate::ssh_test_server::{client_over, serve, TestClient, TestServer};
use std::time::Duration;
use tokio::time::Instant;

/// What the flooding tab prints.
const FLOOD: usize = 64 << 20;
/// How fast the page draws: what `Outflow` assumes until it knows.
const PAGE_RATE: f64 = 8_000_000.0;

/// A login to the jump host over `link`: a tab's own (`None`), or a shared
/// one with this window (`jump_pool::tune`, with its window replaced).
async fn jump_login(link: &ShapedLink, window: Option<u32>) -> client::Handle<TestClient> {
    let (c, s) = link.dial();
    serve(TestServer::default(), |_| {}, s);
    let config = match window {
        Some(w) => {
            let mut c = crate::jump_pool::tune(crate::build_ssh_client_config());
            c.window_size = w;
            c
        }
        None => crate::build_ssh_client_config(),
    };
    client_over(c, config, TestClient::default()).await.expect("the jump host logs in")
}

/// A tab's own session on the server behind the jump host, over a channel
/// on `login`, as initiate_connection opens it.
async fn tab_session(login: &client::Handle<TestClient>, config: client::Config) -> client::Handle<TestClient> {
    let stream = crate::hop_channel(login, "target", 22).await.expect("a channel through the jump host");
    client_over(stream, config, TestClient::default()).await.expect("the server behind it logs in")
}

/// A terminal running `command`, through the real pump, to a page that draws
/// at `PAGE_RATE`: keys in, and when each batch came out.
struct Term {
    keys: mpsc::Sender<TerminalCommand>,
    out: mpsc::UnboundedReceiver<Instant>,
    pump: tokio::task::JoinHandle<bool>,
    _resize: tokio::sync::watch::Sender<PtySize>,
}

async fn terminal(session: &client::Handle<TestClient>, command: &str) -> Term {
    let channel = session.channel_open_session().await.unwrap();
    channel.exec(false, command.to_string()).await.unwrap();
    let (keys, rx) = mpsc::channel::<TerminalCommand>(32);
    let (resize, resize_rx) = tokio::sync::watch::channel(PtySize { cols: 80, rows: 24 });
    let (out_tx, out) = mpsc::unbounded_channel::<Instant>();
    let (draw_tx, mut draw_rx) = mpsc::unbounded_channel::<usize>();
    let page = Arc::new(PageDrawn::default());
    // The page: draws what it's handed, one batch after another, at its pace.
    let drawer = Arc::clone(&page);
    tokio::spawn(async move {
        while let Some(n) = draw_rx.recv().await {
            tokio::time::sleep(Duration::from_secs_f64(n as f64 / PAGE_RATE)).await;
            drawer.add(n as u64);
        }
    });
    let pump = tokio::spawn(pty_pump(channel, rx, resize_rx, page, Vec::new(), move |buf: &mut Vec<u8>| {
        let _ = out_tx.send(Instant::now());
        let _ = draw_tx.send(buf.len());
        buf.clear();
    }));
    Term { keys, out, pump, _resize: resize }
}

/// A key typed every `every`, each echo timed from the key to the batch that
/// brings it back, `count` times or until `until` ends; in milliseconds.
async fn echoes(term: &mut Term, every: Duration, count: usize, until: impl std::future::Future<Output = ()>) -> Vec<f64> {
    tokio::pin!(until);
    let mut took = Vec::new();
    let mut next = Instant::now() + every;
    while took.len() < count {
        tokio::select! {
            _ = tokio::time::sleep_until(next) => {}
            _ = &mut until => break,
        }
        let at = Instant::now();
        term.keys.send(TerminalCommand::Data(vec![b'k'])).await.unwrap();
        let back = tokio::time::timeout(Duration::from_secs(30), term.out.recv()).await.expect("the echo comes back").unwrap();
        took.push((back - at).as_secs_f64() * 1000.0);
        next = (next + every).max(Instant::now());
    }
    took
}

/// (p50, p99) in milliseconds.
fn percentiles(mut v: Vec<f64>) -> (f64, f64) {
    assert!(!v.is_empty());
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    (at(0.5), at(0.99))
}

/// A tab's echo, idle, 200 times: (p50, p99).
async fn idle_echo(shared: bool) -> (f64, f64) {
    let link = ShapedLink::new(LinkSpec::new(100, Duration::from_millis(40), 1 << 20, Queuing::Fair));
    let login = jump_login(&link, shared.then_some(crate::jump_pool::WINDOW)).await;
    // On a shared login, 15 other tabs are open and idle: more than a login
    // carries until its key has `MAX_LOGINS` logins.
    let mut others = Vec::new();
    if shared {
        for _ in 0..15 {
            let s = tab_session(&login, crate::build_ssh_client_config()).await;
            let t = terminal(&s, "echo").await;
            others.push((s, t));
        }
    }
    let session = tab_session(&login, crate::build_ssh_client_config()).await;
    let mut term = terminal(&session, "echo").await;
    let took = echoes(&mut term, Duration::from_millis(60), 200, std::future::pending()).await;
    percentiles(took)
}

#[tokio::test(start_paused = true)]
async fn echo_through_a_shared_login_is_as_fast_as_through_its_own() {
    let own = idle_echo(false).await;
    let shared = idle_echo(true).await;
    println!("idle echo, 100 Mbit/s, 40 ms: own login p50 {:.2} ms p99 {:.2} ms; shared (16 tabs) p50 {:.2} ms p99 {:.2} ms", own.0, own.1, shared.0, shared.1);
    assert!(shared.0 <= own.0 + 1.0, "p50: shared {:.2} ms, own {:.2} ms", shared.0, own.0);
    assert!(shared.1 <= own.1 + 1.0, "p99: shared {:.2} ms, own {:.2} ms", shared.1, own.1);
}

/// A flooding tab and an echoing one on `link`, on one shared login or a
/// login each: the echo's (p50, p99) while the flood runs, and the flood's
/// speed (bytes a second).
async fn flood_beside_echo(spec: LinkSpec, shared: Option<u32>) -> ((f64, f64), f64) {
    let link = ShapedLink::new(spec);
    let (flood_login, echo_login) = if shared.is_some() {
        let l = jump_login(&link, shared).await;
        (l, None)
    } else {
        (jump_login(&link, None).await, Some(jump_login(&link, None).await))
    };
    let flood_session = tab_session(&flood_login, crate::build_ssh_client_config()).await;
    let echo_session = tab_session(echo_login.as_ref().unwrap_or(&flood_login), crate::build_ssh_client_config()).await;
    let mut echo = terminal(&echo_session, "echo").await;
    let started = Instant::now();
    let flood = terminal(&flood_session, &format!("flood {FLOOD}")).await;
    let Term { pump, keys: flood_keys, .. } = flood;
    let done = Arc::new(tokio::sync::Notify::new());
    let ended = Arc::clone(&done);
    let finish = tokio::spawn(async move {
        pump.await.unwrap();
        let at = Instant::now();
        ended.notify_one();
        at
    });
    // The flood fills the link first.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let took = echoes(&mut echo, Duration::from_millis(100), usize::MAX, done.notified()).await;
    let ended_at = finish.await.unwrap();
    drop(flood_keys);
    let speed = FLOOD as f64 / (ended_at - started).as_secs_f64();
    assert!(took.len() >= 20, "{} echoes while the flood ran", took.len());
    (percentiles(took), speed)
}

/// What the window allows a flood at `rtt`, or the link, whichever is less:
/// what the flooding tab should keep 80% of.
fn flood_bound(spec: &LinkSpec) -> f64 {
    let rtt = (spec.delay * 2).as_secs_f64();
    (spec.rate as f64).min(f64::from(crate::jump_pool::WINDOW) / rtt)
}

/// A FIFO bottleneck, 20 Mbit/s with a 1 MiB buffer (a home router, a VPN):
/// on logins of their own, the flood's 8 MiB window keeps the queue full in
/// front of the echo; on a shared one the queue is at most its 512 KiB.
#[tokio::test(start_paused = true)]
async fn an_idle_tab_echoes_at_speed_while_its_neighbour_floods_on_a_fifo_link() {
    let spec = LinkSpec::new(20, Duration::from_millis(40), 1 << 20, Queuing::Fifo);
    let (own, own_speed) = flood_beside_echo(spec, None).await;
    let (shared, speed) = flood_beside_echo(spec, Some(crate::jump_pool::WINDOW)).await;
    println!(
        "FIFO 20 Mbit/s, 40 ms, 1 MiB buffer: echo beside a flood: own logins p50 {:.1} ms p99 {:.1} ms (flood {:.2} MB/s); shared p50 {:.1} ms p99 {:.1} ms (flood {:.2} MB/s)",
        own.0, own.1, own_speed / 1e6, shared.0, shared.1, speed / 1e6
    );
    assert!(shared.1 <= own.1, "p99: shared {:.1} ms, own logins {:.1} ms", shared.1, own.1);
    let bound = flood_bound(&spec);
    assert!(speed >= 0.8 * bound, "the flood ran at {:.2} MB/s, under 80% of {:.2} MB/s", speed / 1e6, bound / 1e6);
}

/// A fair bottleneck, 100 Mbit/s (fq_codel), 20 ms there and back: an echo on
/// a login of its own is a flow of its own, not held up by the flood; on the
/// shared login it waits behind at most the window, 512 KiB, plus a packet:
/// 45 ms at this speed.
#[tokio::test(start_paused = true)]
async fn an_idle_tab_echoes_at_speed_while_its_neighbour_floods_on_a_fair_link() {
    fair_link_flood(Duration::from_millis(20)).await;
}

/// The same at 40 ms there and back. The echo stays within its 45 ms; the
/// flood gets about half of 512 KiB a round trip (6.25 MB/s here), since
/// russh tops a channel's window up only once half of it is used. That is
/// the cost the owner accepted for sharing, which is off by default (#237);
/// a larger window that would lift it breaks the echo bar
/// (`measure_shared_windows`). Held at that, so it can't get worse unseen.
#[tokio::test(start_paused = true)]
async fn an_idle_tab_echoes_at_speed_while_its_neighbour_floods_on_a_fair_link_at_40_ms() {
    fair_link_flood_with(Duration::from_millis(40), ACCEPTED_AT_40_MS).await;
}

/// The share of `flood_bound` a flood through a shared login keeps: the
/// design's bar, and what the owner accepted where russh's window top-up
/// holds it to about half (#237).
const DESIGN_BAR: f64 = 0.8;
const ACCEPTED_AT_40_MS: f64 = 0.45;

async fn fair_link_flood(rtt: Duration) {
    fair_link_flood_with(rtt, DESIGN_BAR).await;
}

async fn fair_link_flood_with(rtt: Duration, share: f64) {
    let spec = LinkSpec::new(100, rtt, 1 << 20, Queuing::Fair);
    let (own, own_speed) = flood_beside_echo(spec, None).await;
    let (shared, speed) = flood_beside_echo(spec, Some(crate::jump_pool::WINDOW)).await;
    println!(
        "fair 100 Mbit/s, {} ms: echo beside a flood: own logins p50 {:.1} ms p99 {:.1} ms (flood {:.2} MB/s); shared p50 {:.1} ms p99 {:.1} ms (flood {:.2} MB/s)",
        rtt.as_millis(), own.0, own.1, own_speed / 1e6, shared.0, shared.1, speed / 1e6
    );
    let allowed = f64::from(crate::jump_pool::WINDOW + crate::jump_pool::MAX_PACKET) / spec.rate as f64 * 1000.0;
    assert!(shared.1 <= own.1 + allowed, "p99: shared {:.1} ms, own logins {:.1} ms + {allowed:.1} ms", shared.1, own.1);
    let bound = flood_bound(&spec);
    assert!(speed >= share * bound, "the flood ran at {:.2} MB/s, under {:.0}% of {:.2} MB/s", speed / 1e6, share * 100.0, bound / 1e6);
}

/// A tab whose session stops reading (the app holds its channel and reads
/// nothing) while its server floods: the shared login's other tabs echo on,
/// behind at most the window while the flood crosses the link, and as if
/// idle once it has. Its backlog is cut at `tunnel::MAX_UNFLUSHED_BYTES`
/// (tunnel.rs's own tests), and the login stays up.
#[tokio::test(start_paused = true)]
async fn a_stalled_tab_never_holds_up_the_shared_connection() {
    let spec = LinkSpec::new(100, Duration::from_millis(40), 1 << 20, Queuing::Fair);
    let idle = idle_echo(true).await;
    let link = ShapedLink::new(spec);
    let login = jump_login(&link, Some(crate::jump_pool::WINDOW)).await;
    // Its session takes all the server sends (a window larger than the
    // flood), and nothing reads its channel: its session stops at once, and
    // what comes waits in the channel's drained queue, past the cap.
    let mut big = crate::build_ssh_client_config();
    big.window_size = 200 << 20;
    let stalled = tab_session(&login, big).await;
    let channel = stalled.channel_open_session().await.unwrap();
    const STALLED_FLOOD: usize = 100 << 20;
    channel.exec(false, format!("flood {STALLED_FLOOD}")).await.unwrap();
    let echo_session = tab_session(&login, crate::build_ssh_client_config()).await;
    let mut echo = terminal(&echo_session, "echo").await;
    let started = Instant::now();
    // While the flood crosses the link (8.4 s at this speed).
    let during = echoes(&mut echo, Duration::from_millis(100), usize::MAX, tokio::time::sleep(Duration::from_secs(6))).await;
    // Once it has.
    tokio::time::sleep_until(started + Duration::from_secs(20)).await;
    let after = echoes(&mut echo, Duration::from_millis(60), 100, std::future::pending()).await;
    let (during, after) = (percentiles(during), percentiles(after));
    println!(
        "a stalled tab on the shared login, 100 Mbit/s fair, 40 ms: echo while its flood crossed p50 {:.1} ms p99 {:.1} ms; after p50 {:.2} ms p99 {:.2} ms (idle p99 {:.2} ms)",
        during.0, during.1, after.0, after.1, idle.1
    );
    let allowed = f64::from(crate::jump_pool::WINDOW + crate::jump_pool::MAX_PACKET) / spec.rate as f64 * 1000.0;
    assert!(during.1 <= idle.1 + allowed, "p99 while it stalled: {:.1} ms", during.1);
    assert!(after.1 <= idle.1 + 1.0, "p99 after: {:.2} ms, idle {:.2} ms", after.1, idle.1);
    assert!(!login.is_closed() && !echo_session.is_closed(), "the shared login stays up");
    drop(channel);
}

/// The flood's speed on a shared login and on one of its own, by round trip,
/// on a fair 100 Mbit/s link. A measure, not a check: it prints, and is run by
/// hand (`cargo test --lib measure_a_flood -- --ignored --nocapture`).
#[tokio::test(start_paused = true)]
#[ignore = "a measure, run by hand"]
async fn measure_a_floods_speed_by_round_trip() {
    for rtt in [5, 10, 20, 40, 80] {
        let spec = LinkSpec::new(100, Duration::from_millis(rtt), 1 << 20, Queuing::Fair);
        let (own, own_speed) = flood_beside_echo(spec, None).await;
        let (shared, speed) = flood_beside_echo(spec, Some(crate::jump_pool::WINDOW)).await;
        println!(
            "fair 100 Mbit/s, {rtt} ms: own logins echo p99 {:.1} ms, flood {:.2} MB/s; shared echo p99 {:.1} ms, flood {:.2} MB/s ({:.0}% of min(link, 512 KiB/RTT) = {:.2} MB/s)",
            own.1, own_speed / 1e6, shared.1, speed / 1e6, 100.0 * speed / flood_bound(&spec), flood_bound(&spec) / 1e6
        );
    }
}

/// Ten tabs on a key's five logins (placed by the pool, as "Logins per jump
/// host" 5 has them), on a fair link: a flood in one tab leaves the tabs on
/// the other logins at the echo of an idle login of their own.
#[tokio::test(start_paused = true)]
async fn a_flood_on_one_login_leaves_the_tabs_on_the_others_at_idle_speed() {
    use crate::jump_pool::{Acquired, Key, Pool, Via};
    let spec = LinkSpec::new(100, Duration::from_millis(40), 1 << 20, Queuing::Fair);
    let idle = idle_echo(false).await;
    let link = ShapedLink::new(spec);
    let pool = Pool::<TestClient>::new();
    let key = Key::new(Via::Direct, crate::hop_policy::queue_key("bastion", 22), "alice".into(), "jump1".into(), crate::jump_pool::digest(&[b"pw"]));
    let gens = Arc::new(Mutex::new(HashMap::new()));
    let mut tabs = Vec::new();
    for i in 0..10 {
        let id = format!("t{i}");
        gens.lock().await.insert(id.clone(), 1);
        let attempt = ConnectAttempt::new(Arc::clone(&gens), id.clone(), 1);
        let (login, lease) = match pool.acquire(key.clone(), &id, &attempt, 5, || {}).await.expect("a login") {
            Acquired::Shared(h, l) => (h, l),
            Acquired::Create(r) => {
                let h = Arc::new(jump_login(&link, Some(crate::jump_pool::WINDOW)).await);
                let l = r.ready(Arc::clone(&h), None, None).expect("kept");
                (h, l)
            }
        };
        let session = tab_session(&login, crate::build_ssh_client_config()).await;
        tabs.push((login, lease, session));
    }
    assert_eq!(link.dials(), 5);
    assert_eq!(pool.census(&key), vec![2; 5]);
    // The first tab floods; the second is on its login too. The other eight
    // are on the other four logins.
    assert!(Arc::ptr_eq(&tabs[0].0, &tabs[1].0));
    assert!(tabs[2..].iter().all(|(l, _, _)| !Arc::ptr_eq(l, &tabs[0].0)));
    let flood = terminal(&tabs[0].2, &format!("flood {FLOOD}")).await;
    let mut others = Vec::new();
    for (_, _, session) in &tabs[2..] {
        others.push(terminal(session, "echo").await);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let took = futures::future::join_all(others.iter_mut().map(|t| echoes(t, Duration::from_millis(100), 30, std::future::pending()))).await;
    assert!(!flood.pump.is_finished(), "measured while the flood ran");
    let beside = percentiles(took.into_iter().flatten().collect());
    println!(
        "a flood on one of five logins, fair 100 Mbit/s, 40 ms: echo on the other logins p50 {:.2} ms p99 {:.2} ms (idle own login p99 {:.2} ms)",
        beside.0, beside.1, idle.1
    );
    assert!(beside.1 <= idle.1 + 1.0, "p99 on the other logins {:.2} ms, idle {:.2} ms", beside.1, idle.1);
}

/// Each shared window tried against logins of their own: the flood's speed
/// and the echo beside it, on a fair 100 Mbit/s link at 20, 40 and 80 ms and
/// a FIFO 20 Mbit/s one at 40 ms. A measure, run by hand
/// (`cargo test --lib measure_shared_windows -- --ignored --nocapture`).
#[tokio::test(start_paused = true)]
#[ignore = "a measure, run by hand"]
async fn measure_shared_windows() {
    let links = [
        ("fair 100 Mbit/s 20 ms", LinkSpec::new(100, Duration::from_millis(20), 1 << 20, Queuing::Fair)),
        ("fair 100 Mbit/s 40 ms", LinkSpec::new(100, Duration::from_millis(40), 1 << 20, Queuing::Fair)),
        ("fair 100 Mbit/s 80 ms", LinkSpec::new(100, Duration::from_millis(80), 1 << 20, Queuing::Fair)),
        ("FIFO 20 Mbit/s 40 ms", LinkSpec::new(20, Duration::from_millis(40), 1 << 20, Queuing::Fifo)),
    ];
    for (name, spec) in links {
        let (own, own_speed) = flood_beside_echo(spec, None).await;
        println!("{name}: own logins: echo p99 {:.1} ms, flood {:.2} MB/s", own.1, own_speed / 1e6);
        for w in [512u32 << 10, 1 << 20, 2 << 20, 3 << 20, 4 << 20, 8 << 20] {
            let (shared, speed) = flood_beside_echo(spec, Some(w)).await;
            println!(
                "{name}: shared {} KiB: echo p99 {:.1} ms ({:+.1}), flood {:.2} MB/s ({:.1}% of own)",
                w >> 10, shared.1, shared.1 - own.1, speed / 1e6, 100.0 * speed / own_speed
            );
        }
    }
}

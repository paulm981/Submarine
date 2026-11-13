//! Asking for a server by name: what's found, what's refused, and that the
//! AI hears the same whether you said no or there was nothing to share.

use super::*;
use serde_json::json;

fn vault() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE servers (id INTEGER PRIMARY KEY, name TEXT, host TEXT, port INTEGER, username TEXT, credential_id INTEGER, folder_id INTEGER, protocol TEXT, deleted INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE folders (id INTEGER PRIMARY KEY, name TEXT, parent_id INTEGER);
         CREATE TABLE credentials (id INTEGER PRIMARY KEY, name TEXT, username TEXT);
         INSERT INTO folders (id, name, parent_id) VALUES (1, 'Production', NULL), (2, 'Web', 1), (3, 'Lab', NULL);
         INSERT INTO credentials (id, name, username) VALUES (1, 'deploy key', 'deploy');
         INSERT INTO servers (id, name) VALUES (1, 'web1'), (2, 'db-01'), (5, 'école');
         INSERT INTO servers (id, name, host, port, username, credential_id, folder_id) VALUES
           (3, 'twin', '10.0.0.3', 22, 'root', NULL, 2), (4, 'Twin', '10.0.0.4', 2222, '', 1, NULL);
         INSERT INTO servers (id, name, protocol) VALUES (6, 'win', 'rdp'), (7, 'desk', 'vnc'), (8, 'switch', 'telnet'), (9, 'ssh1', 'ssh');
         INSERT INTO servers (id, name, deleted) VALUES (10, 'gone', 1);",
    )
    .unwrap();
    conn
}

#[test]
fn only_an_exact_name_finds_a_server_ignoring_case() {
    let conn = vault();
    let find = |conn: &rusqlite::Connection, name: &str| find(conn, name, &HashSet::new());
    assert_eq!(find(&conn, "web1"), Found::One(1, "web1".into()));
    assert_eq!(find(&conn, "DB-01"), Found::One(2, "db-01".into()));
    for partial in ["web", "web1 ", "%", "_eb1", "db", ""] {
        assert_eq!(find(&conn, partial), Found::None, "{partial:?}");
    }
    assert!(matches!(find(&conn, "twin"), Found::Many { ref candidates, more: 0 } if candidates.len() == 2));
    // Case in any alphabet, not only ASCII.
    assert_eq!(find(&conn, "ÉCOLE"), Found::One(5, "école".into()));
    assert_eq!(find(&conn, "ssh1"), Found::One(9, "ssh1".into()));
}

#[test]
fn a_server_that_cant_be_shared_isnt_found() {
    let conn = vault();
    // RDP, VNC and telnet have no SSH session to share; a deleted one is gone.
    for name in ["win", "desk", "switch", "gone"] {
        assert_eq!(find(&conn, name, &HashSet::new()), Found::None, "{name}");
    }
}

#[test]
fn a_name_is_checked_before_anything_is_asked() {
    assert_eq!(check_name(Some(&json!("  web1 "))).unwrap(), "web1");
    for bad in [
        None,
        Some(json!(null)),
        Some(json!(5)),
        Some(json!("")),
        Some(json!("   ")),
        Some(json!("a\nb")),
        Some(json!("web1\u{202e}")),
        Some(json!("x".repeat(MAX_NAME + 1))),
        // Hidden characters the page hides too: a word joiner, a soft
        // hyphen, an Arabic mark, a tag, a Hangul filler, a variation selector.
        Some(json!("prod\u{2060}")),
        Some(json!("pr\u{ad}od")),
        Some(json!("prod\u{61c}")),
        Some(json!("prod\u{e0041}")),
        Some(json!("prod\u{3164}")),
        Some(json!("prod\u{fe0f}")),
    ] {
        assert!(check_name(bad.as_ref()).is_err(), "{bad:?}");
    }
    assert!(check_name(Some(&json!("é".repeat(MAX_NAME)))).is_ok(), "counted in characters");
}

fn state() -> AiState {
    state_on(vault(), &[])
}

/// A test's AI tab, finding names in `vault`, with the tabs of the saved
/// servers `open` connected.
fn state_on(vault: rusqlite::Connection, open: &[i64]) -> AiState {
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), None);
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    let (conn, open) = (Arc::new(Mutex::new(vault)), Arc::new(open.iter().copied().collect::<HashSet<i64>>()));
    s.set_server_lookup(Arc::new(move |name| {
        let found = find(&conn.lock().unwrap(), &name, &open);
        Box::pin(async move { found })
    }));
    s
}

/// Ask for `name` and wait until its card is there.
async fn ask(s: &AiState, name: &str) -> (tokio::task::JoinHandle<ToolResult>, AskView) {
    let (s2, args) = (s.clone(), json!({ "name": name }));
    let before = s.snapshot().server_asks.len();
    let h = tokio::spawn(async move { s2.request_server("local-1", &args, watch::channel(false).1).await });
    for _ in 0..200 {
        if let Some(v) = s.snapshot().server_asks.get(before) {
            return (h, v.clone());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no card for {name}");
}

#[tokio::test]
async fn no_and_no_such_server_sound_the_same_to_the_ai() {
    let s = state();
    let (h, card) = ask(&s, "nothing-here").await;
    assert_eq!((card.server_id, card.many), (None, false));
    s.decline_ask(card.id).unwrap();
    let unknown = h.await.unwrap();
    let (h, card) = ask(&s, "web1").await;
    assert_eq!(card.server_name.as_deref(), Some("web1"));
    s.decline_ask(card.id).unwrap();
    let declined = h.await.unwrap();
    assert!(unknown.is_error && declined.is_error);
    assert_eq!(unknown.text, declined.text);
    assert_eq!(unknown.text, NOT_SHARED);
    // Two servers by the name: the card lists them, and Don't share is a
    // no, in the same words.
    let (h, card) = ask(&s, "TWIN").await;
    assert!(card.many && card.server_id.is_none() && card.candidates.len() == 2);
    s.decline_ask(card.id).unwrap();
    let many = h.await.unwrap();
    assert!(many.is_error);
    assert_eq!(many.text, NOT_SHARED);
}

#[tokio::test]
async fn yes_shares_it_once_it_connects() {
    let s = state();
    let (h, card) = ask(&s, "DB-01").await;
    assert_eq!(card.server_id, Some(2));
    let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = tries.clone();
    s.share_ask(card.id, None, move |id| {
        assert_eq!(id, 2);
        let n = t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Not connected at first: it's still opening.
        async move { (n >= 1).then(|| ("session-2".to_string(), Some("deploy".to_string()))) }
    })
    .await
    .unwrap();
    let r = h.await.unwrap();
    assert!(!r.is_error && r.text.contains("as db-01"), "{}", r.text);
    let snap = s.snapshot();
    let shared = &snap.tabs[0].servers[0];
    assert_eq!((shared.name.as_str(), shared.server_id, shared.user.as_deref()), ("db-01", Some(2), Some("deploy")));
    assert!(snap.server_asks.is_empty());
    // Asked again: it's there already, nothing to ask.
    let again = s.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await;
    assert!(!again.is_error && again.text.contains("already shared"), "{}", again.text);
    assert!(s.snapshot().server_asks.is_empty());
}

#[tokio::test]
async fn a_server_shared_under_another_name_is_found_by_its_id() {
    let s = state();
    s.share_as("local-1", "sess-2", "prod", None, Some(2)).unwrap();
    let r = s.request_server("local-1", &json!({ "name": "DB-01" }), watch::channel(false).1).await;
    assert!(!r.is_error && r.text.contains("already shared with you as prod"), "{}", r.text);
    assert!(s.snapshot().server_asks.is_empty());
}

#[tokio::test]
async fn a_quick_connect_by_the_same_name_isnt_the_saved_server() {
    let s = state();
    // A quick connect, shared as "db-01": another machine than saved db-01.
    s.share_as("local-1", "quick", "db-01", None, None).unwrap();
    let (h, card) = ask(&s, "db-01").await;
    assert_eq!(card.server_id, Some(2));
    s.decline_ask(card.id).unwrap();
    assert_eq!(h.await.unwrap().text, NOT_SHARED);
}

/// Share, then the tab connects only once `up` is set.
fn connects_when(up: &Arc<std::sync::atomic::AtomicBool>) -> impl Fn(i64) -> std::future::Ready<Option<(String, Option<String>)>> {
    let up = up.clone();
    move |_| std::future::ready(up.load(std::sync::atomic::Ordering::SeqCst).then(|| ("session-1".to_string(), None)))
}

#[tokio::test]
async fn stop_while_it_connects_shares_nothing() {
    let s = state();
    let (h, card) = ask(&s, "web1").await;
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s2, f) = (s.clone(), connects_when(&up));
    let share = tokio::spawn(async move { s2.share_ask(card.id, None, f).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The card stays while it connects, marked so, and still counts as the
    // one waiting.
    let snap = s.snapshot();
    assert!(snap.server_asks.len() == 1 && snap.server_asks[0].connecting);
    let again = s.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await;
    assert!(again.is_error && again.text.contains("already waits"));
    s.stop_tab("local-1");
    up.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(share.await.unwrap().is_err());
    assert!(s.snapshot().tabs[0].servers.is_empty(), "Stop wins over a Share still connecting");
    let r = h.await.unwrap();
    assert!(r.is_error && r.text.contains("stopped it"), "{}", r.text);
}

#[tokio::test]
async fn dont_share_while_it_connects_shares_nothing() {
    let s = state();
    let (h, card) = ask(&s, "web1").await;
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s2, f) = (s.clone(), connects_when(&up));
    let share = tokio::spawn(async move { s2.share_ask(card.id, None, f).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    s.decline_ask(card.id).unwrap();
    up.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(share.await.unwrap().is_err());
    assert!(s.snapshot().tabs[0].servers.is_empty());
    assert_eq!(h.await.unwrap().text, NOT_SHARED);
}

#[tokio::test]
async fn withdrawn_after_share_shares_nothing_and_blames_nothing() {
    let s = state();
    let (cancel_tx, cancel) = watch::channel(false);
    let s2 = s.clone();
    let h = tokio::spawn(async move { s2.request_server("local-1", &json!({ "name": "web1" }), cancel).await });
    wait_for_card(&s).await;
    let card = s.snapshot().server_asks[0].clone();
    let up = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s3, f) = (s.clone(), connects_when(&up));
    let share = tokio::spawn(async move { s3.share_ask(card.id, None, f).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel_tx.send(true).unwrap();
    assert!(h.await.unwrap().text.contains("withdrawn"));
    // Already connected by the time it looks again: still not shared.
    up.store(true, std::sync::atomic::Ordering::SeqCst);
    let why = share.await.unwrap().unwrap_err();
    assert!(!why.contains("didn't connect"), "{why}");
    assert!(s.snapshot().tabs[0].servers.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_server_that_never_connects_isnt_shared_and_the_ai_hears_why() {
    let s = state();
    let (h, card) = ask(&s, "web1").await;
    let never = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let why = s.share_ask(card.id, None, connects_when(&never)).await.unwrap_err();
    assert!(why.contains("didn't connect within 90 seconds"), "{why}");
    let r = h.await.unwrap();
    assert!(r.is_error && r.text.contains("didn't connect"), "{}", r.text);
    assert!(s.snapshot().tabs[0].servers.is_empty() && s.snapshot().server_asks.is_empty());
}

#[tokio::test]
async fn two_at_once_make_one_card() {
    let s = state();
    let (a, b) = (s.clone(), s.clone());
    let h1 = tokio::spawn(async move { a.request_server("local-1", &json!({ "name": "web1" }), watch::channel(false).1).await });
    let h2 = tokio::spawn(async move { b.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(s.snapshot().server_asks.len(), 1);
    s.stop_tab("local-1");
    let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());
    assert!(r1.text.contains("already waits") || r2.text.contains("already waits"), "{} / {}", r1.text, r2.text);
}

#[tokio::test]
async fn a_request_for_a_server_counts_against_the_rate_and_the_waiting() {
    let s = state();
    // Asked and answered, again and again: the rate stops it.
    let mut refused = None;
    for i in 0..(RATE_MAX + 1) {
        let s2 = s.clone();
        let h = tokio::spawn(async move { s2.request_server("local-1", &json!({ "name": "web1" }), watch::channel(false).1).await });
        let mut card = None;
        for _ in 0..100 {
            if let Some(c) = s.snapshot().server_asks.first().cloned() {
                card = Some(c);
                break;
            }
            if h.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        match card {
            Some(c) => {
                s.decline_ask(c.id).unwrap();
                let _ = h.await;
            }
            None => {
                refused = Some((i, h.await.unwrap()));
                break;
            }
        }
    }
    let (i, r) = refused.expect("the rate limit refused one");
    assert_eq!(i, RATE_MAX);
    assert!(r.is_error && r.text.contains("Too many requests"), "{}", r.text);
}

#[tokio::test]
async fn a_waiting_request_for_a_server_counts_as_one_waiting() {
    let s = state();
    let (tx, _rx) = oneshot::channel();
    {
        let mut sh = s.lock();
        sh.server_asks.push(Ask {
            view: AskView {
                id: 99,
                tab_id: "local-1".into(),
                tab_label: "Claude".into(),
                asked: "web1".into(),
                server_id: Some(1),
                server_name: Some("web1".into()),
                many: false,
                candidates: Vec::new(),
                more: 0,
                asked_at: String::new(),
                connecting: false,
            },
            decide: tx,
        });
        assert_eq!(waiting_requests(&sh, "local-1"), 1);
        assert_eq!(waiting_requests(&sh, "local-2"), 0);
    }
}

async fn wait_for_card(s: &AiState) {
    for _ in 0..200 {
        if !s.snapshot().server_asks.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no card");
}

#[tokio::test]
async fn one_waits_at_a_time_and_it_ends_with_the_ai_tab() {
    let s = state();
    let (h, _card) = ask(&s, "web1").await;
    let second = s.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await;
    assert!(second.is_error && second.text.contains("already waits"));
    s.stop_tab("local-1");
    let r = h.await.unwrap();
    assert!(r.is_error && r.text.contains("stopped it"), "{}", r.text);
    assert!(s.snapshot().server_asks.is_empty());
    // Ending the AI tab ends it too.
    let s = state();
    let (h, _card) = ask(&s, "web1").await;
    s.end_tab("local-1");
    let r = h.await.unwrap();
    assert!(r.is_error && r.text.contains("access ended"), "{}", r.text);
}

#[tokio::test]
async fn a_bad_name_never_reaches_you() {
    let s = state();
    let r = s.request_server("local-1", &json!({ "name": "web1\u{202e}x" }), watch::channel(false).1).await;
    assert!(r.is_error);
    assert!(s.snapshot().server_asks.is_empty());
}

// The call from the original project's code into this file (fork_hooks.rs).
#[test]
fn the_hooks_are_in_place() {
    crate::fork_hooks::assert_hooks("lib.rs", include_str!("../../lib.rs"), &[("run", "ai::ai_answer_server_ask", 1)]);
}

#[tokio::test]
async fn stop_as_it_connects_shares_nothing() {
    // Stop lands while the connect check runs, which then finds it up: the
    // last look, as the request is taken off the list, still wins.
    let s = state();
    let (h, card) = ask(&s, "web1").await;
    let s2 = s.clone();
    let r = s
        .share_ask(card.id, None, move |_| {
            s2.stop_tab("local-1");
            std::future::ready(Some(("session-1".to_string(), None)))
        })
        .await;
    assert!(r.is_err());
    assert!(s.snapshot().tabs[0].servers.is_empty());
    assert!(h.await.unwrap().is_error);
}

#[tokio::test]
async fn an_ai_gone_without_a_word_gets_nothing_shared() {
    // Its call dropped (the connection closed) without withdrawing: the card
    // is still there, but nobody waits on it any more.
    let s = state();
    let (h, card) = ask(&s, "web1").await;
    h.abort();
    let _ = h.await;
    let r = s.share_ask(card.id, None, |_| std::future::ready(Some(("session-1".to_string(), None)))).await;
    assert!(r.is_err());
    assert!(s.snapshot().tabs[0].servers.is_empty());
    assert!(s.snapshot().server_asks.is_empty(), "and the card goes");
}

/// A vault with `n` saved servers named "app" (ids 101 on), spread over
/// folders, and one "solo".
fn twins(n: i64) -> rusqlite::Connection {
    let conn = vault();
    for i in 0..n {
        let folder = [None, Some(1), Some(2), Some(3)][(i % 4) as usize];
        conn.execute(
            "INSERT INTO servers (id, name, host, port, username, folder_id) VALUES (?1, ?2, ?3, 22, 'ops', ?4)",
            rusqlite::params![101 + i, if i % 2 == 0 { "app" } else { "APP" }, format!("10.1.0.{i}"), folder],
        )
        .unwrap();
    }
    conn
}

fn many(found: Found) -> (Vec<Candidate>, usize) {
    match found {
        Found::Many { candidates, more } => (candidates, more),
        other => panic!("not several: {other:?}"),
    }
}

#[test]
fn several_with_the_name_are_listed_with_where_they_are() {
    let conn = vault();
    let (list, more) = many(find(&conn, "twin", &HashSet::new()));
    assert_eq!(more, 0);
    // The top level first, then by folder path; the user from the
    // credential where the server has none.
    let shown: Vec<_> = list.iter().map(|c| (c.id, c.folder.as_str(), c.user.as_str(), c.host.as_str(), c.port, c.open)).collect();
    assert_eq!(shown, [(4, "", "deploy", "10.0.0.4", Some(2222), false), (3, "Production / Web", "root", "10.0.0.3", Some(22), false)]);
    assert_eq!((list[0].name.as_str(), list[1].name.as_str()), ("Twin", "twin"));
}

#[test]
fn none_open_lists_them_by_folder_then_name_at_most_eight() {
    let (list, more) = many(find(&twins(11), "app", &HashSet::new()));
    assert_eq!((list.len(), more), (MAX_SHOWN, 3));
    assert!(list.iter().all(|c| !c.open));
    let folders: Vec<_> = list.iter().map(|c| c.folder.as_str()).collect();
    let mut sorted = folders.clone();
    sorted.sort_by_key(|f| f.to_lowercase());
    assert_eq!(folders, sorted);
    assert_eq!(folders[0], "");
}

#[test]
fn one_open_comes_first_even_past_the_eighth() {
    // 111 would be cut, by folder order, if it weren't open.
    let open = HashSet::from([111]);
    let (list, more) = many(find(&twins(11), "app", &open));
    assert_eq!((list.len(), more), (MAX_SHOWN, 3));
    assert_eq!(list[0].id, 111);
    assert!(list[0].open && list[1..].iter().all(|c| !c.open));
}

#[test]
fn several_open_come_first_in_folder_order() {
    // 109 at the top level, 104 and 108 in Lab, 110 in Production: open,
    // and in folder order, then by name, then by id.
    let open = HashSet::from([110, 104, 109, 108]);
    let (list, _) = many(find(&twins(11), "app", &open));
    let first: Vec<_> = list.iter().take(4).map(|c| (c.id, c.open, c.folder.as_str())).collect();
    assert_eq!(first, [(109, true, ""), (104, true, "Lab"), (108, true, "Lab"), (110, true, "Production")]);
    assert!(list[4..].iter().all(|c| !c.open));
}

#[test]
fn a_folder_loop_or_a_deep_path_ends() {
    let conn = vault();
    conn.execute_batch(
        "UPDATE folders SET parent_id = 2 WHERE id = 1;
         INSERT INTO servers (id, name, host, folder_id) VALUES (20, 'loop', 'a', 2), (21, 'loop', 'b', NULL);",
    )
    .unwrap();
    let (list, _) = many(find(&conn, "loop", &HashSet::new()));
    assert_eq!(list[1].folder, "Production / Web");
    let deep: HashMap<i64, (String, Option<i64>)> = (0..200).map(|i| (i, ("f".repeat(10), Some(i + 1)))).collect();
    let path = folder_path(&deep, Some(0));
    assert!(path.chars().count() <= MAX_FIELD && path.starts_with('…'), "{path}");
}

/// Sharing on a card, with the tab found connected once `connected`
/// has been asked `after` times; the ids it was asked for.
async fn share_picked(s: &AiState, id: u64, pick: Option<i64>, after: usize) -> (Result<(), String>, Vec<i64>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let a = asked.clone();
    let r = s
        .share_ask(id, pick, move |server_id| {
            let mut a = a.lock().unwrap();
            a.push(server_id);
            let n = a.len();
            std::future::ready((n > after).then(|| (format!("session-{server_id}"), Some("ops".to_string()))))
        })
        .await;
    let asked = asked.lock().unwrap().clone();
    (r, asked)
}

#[tokio::test]
async fn a_picked_one_is_opened_waited_for_and_shared_as_for_one() {
    let s = state_on(twins(3), &[]);
    let (h, card) = ask(&s, "app").await;
    assert_eq!(card.candidates.len(), 3);
    let (r, asked) = share_picked(&s, card.id, Some(102), 1).await;
    r.unwrap();
    // Not connected at first: waited for, as for a single match.
    assert_eq!(asked, [102, 102]);
    let said = h.await.unwrap();
    assert!(!said.is_error);
    // The same words as for a single match, and nothing about the others.
    assert_eq!(said.text, "The user shared it with you as APP. Its own approval level applies; call list_shared to see it.");
    let snap = s.snapshot();
    let shared = &snap.tabs[0].servers[0];
    assert_eq!((shared.server_id, shared.session_id.as_str()), (Some(102), "session-102"));
    assert!(snap.server_asks.is_empty());
}

#[tokio::test]
async fn one_open_now_is_shared_from_its_tab_at_once() {
    let s = state_on(twins(3), &[103]);
    let (h, card) = ask(&s, "app").await;
    assert!(card.candidates[0].open && card.candidates[0].id == 103);
    let (r, asked) = share_picked(&s, card.id, Some(103), 0).await;
    r.unwrap();
    assert_eq!(asked, [103], "connected already: no wait");
    assert!(h.await.unwrap().text.starts_with("The user shared it with you as app."));
}

#[tokio::test]
async fn a_server_the_request_didnt_find_is_refused() {
    let s = state_on(twins(3), &[1]);
    let (h, card) = ask(&s, "app").await;
    // Another saved server, open or not, or one that isn't saved: refused,
    // without looking for its tab, and the card stays as it was.
    for other in [1, 2, 104, -1] {
        let (r, asked) = share_picked(&s, card.id, Some(other), 0).await;
        assert_eq!(r.unwrap_err(), NOT_A_CANDIDATE, "{other}");
        assert!(asked.is_empty());
    }
    let snap = s.snapshot();
    assert!(snap.tabs[0].servers.is_empty());
    let left = &snap.server_asks[0];
    assert!(!left.connecting && left.server_id.is_none());
    // And on a card for one, only that one.
    s.decline_ask(card.id).unwrap();
    assert_eq!(h.await.unwrap().text, NOT_SHARED);
    let (h, card) = ask(&s, "web1").await;
    let (r, asked) = share_picked(&s, card.id, Some(2), 0).await;
    assert_eq!(r.unwrap_err(), NOT_A_CANDIDATE);
    assert!(asked.is_empty());
    let (r, asked) = share_picked(&s, card.id, Some(1), 0).await;
    r.unwrap();
    assert_eq!(asked, [1]);
    assert!(h.await.unwrap().text.contains("as web1"));
}

#[tokio::test]
async fn share_without_a_pick_on_several_is_a_no_in_the_same_words() {
    let s = state_on(twins(3), &[]);
    let (h, card) = ask(&s, "nothing-here").await;
    s.decline_ask(card.id).unwrap();
    let nothing = h.await.unwrap().text;
    let (h, card) = ask(&s, "app").await;
    let (r, asked) = share_picked(&s, card.id, None, 0).await;
    assert!(r.is_err() && asked.is_empty());
    let dismissed = h.await.unwrap();
    assert!(dismissed.is_error);
    assert_eq!(dismissed.text.as_bytes(), NOT_SHARED.as_bytes());
    assert_eq!(dismissed.text, nothing);
    assert!(s.snapshot().tabs[0].servers.is_empty());
}

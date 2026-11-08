//! Asking for a server by name: what's found, what's refused, and that the
//! AI hears the same whether you said no or there was nothing to share.

use super::*;
use serde_json::json;

fn vault() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE servers (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO servers VALUES (1, 'web1'), (2, 'db-01'), (3, 'twin'), (4, 'Twin');",
    )
    .unwrap();
    conn
}

#[test]
fn only_an_exact_name_finds_a_server_ignoring_case() {
    let conn = vault();
    assert_eq!(find(&conn, "web1"), Found::One(1, "web1".into()));
    assert_eq!(find(&conn, "DB-01"), Found::One(2, "db-01".into()));
    for partial in ["web", "web1 ", "%", "_eb1", "db", ""] {
        assert_eq!(find(&conn, partial), Found::None, "{partial:?}");
    }
    assert_eq!(find(&conn, "twin"), Found::Many);
}

#[test]
fn a_name_is_checked_before_anything_is_asked() {
    assert_eq!(check_name(Some(&json!("  web1 "))).unwrap(), "web1");
    for bad in [None, Some(json!(null)), Some(json!(5)), Some(json!("")), Some(json!("   ")), Some(json!("a\nb")), Some(json!("web1\u{202e}")), Some(json!("x".repeat(MAX_NAME + 1)))] {
        assert!(check_name(bad.as_ref()).is_err(), "{bad:?}");
    }
    assert!(check_name(Some(&json!("é".repeat(MAX_NAME)))).is_ok(), "counted in characters");
}

fn state() -> AiState {
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), None);
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    let conn = Mutex::new(vault());
    s.set_server_lookup(Arc::new(move |name| find(&conn.lock().unwrap(), name)));
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
    // Two servers by the name: the card says so, and nothing is shared.
    let (h, card) = ask(&s, "TWIN").await;
    assert!(card.many && card.server_id.is_none());
    assert!(s.share_ask(card.id, |_| async { None }).await.is_err());
    assert_eq!(h.await.unwrap().text, NOT_SHARED);
}

#[tokio::test]
async fn yes_shares_it_once_it_connects() {
    let s = state();
    let (h, card) = ask(&s, "DB-01").await;
    assert_eq!(card.server_id, Some(2));
    let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = tries.clone();
    s.share_ask(card.id, move |id| {
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
    // Asked again, by either name: it's there already, nothing to ask.
    let again = s.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await;
    assert!(!again.is_error && again.text.contains("already shared"));
}

#[tokio::test]
async fn one_waits_at_a_time_and_it_ends_with_the_ai_tab() {
    let s = state();
    let (h, _card) = ask(&s, "web1").await;
    let second = s.request_server("local-1", &json!({ "name": "db-01" }), watch::channel(false).1).await;
    assert!(second.is_error && second.text.contains("already waits"));
    s.stop_tab("local-1");
    let r = h.await.unwrap();
    assert!(r.is_error && r.text.contains("access ended"), "{}", r.text);
    assert!(s.snapshot().server_asks.is_empty());
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

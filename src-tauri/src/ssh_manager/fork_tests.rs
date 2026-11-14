//! The fork's tests of what it adds to ssh_manager.rs: a key that can't be
//! checked without an open profile, terminals closed while they open, and
//! one host-key question for several sessions. Moved here out of the
//! original project's `mod tests` (#215), so a sync doesn't conflict on them.

use super::*;

// No profile open: the key can't be checked, so it's refused with that
// reason, never asked about as if it were new.
#[test]
fn no_open_profile_means_the_key_cant_be_checked() {
    let none = std::sync::Mutex::new(None);
    assert_eq!(keys_on_file(&none, "h", 22, "FP").unwrap_err(), "no profile is open, so the server's key can't be checked");
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE known_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT, port INTEGER, fingerprint TEXT, key_type TEXT);")
        .unwrap();
    let open = std::sync::Mutex::new(Some(conn));
    assert_eq!(keys_on_file(&open, "h", 22, "FP").unwrap(), HostKeyVerdict::New);
    // A broken table fails closed too.
    open.lock().unwrap().as_ref().unwrap().execute_batch("DROP TABLE known_hosts;").unwrap();
    assert!(keys_on_file(&open, "h", 22, "FP").is_err());
}

#[test]
fn a_terminal_closed_while_opening_is_not_registered() {
    let mut t = TerminalOpenings::default();
    let n = t.begin("s-term-0");
    t.close("s-term-0");
    assert!(!t.finish("s-term-0", n));
    // Nothing left behind for the next open of that terminal.
    let m = t.begin("s-term-0");
    assert!(t.finish("s-term-0", m));
    assert!(t.opening.is_empty());
}

#[test]
fn a_terminal_left_open_is_registered_once() {
    let mut t = TerminalOpenings::default();
    let n = t.begin("a");
    let other = t.begin("b");
    assert!(t.finish("a", n));
    // A late close of another terminal, or a second finish, changes nothing.
    t.close("c");
    assert!(!t.finish("a", n));
    assert!(t.finish("b", other));
}

#[test]
fn a_newer_open_of_the_same_terminal_wins() {
    let mut t = TerminalOpenings::default();
    let old = t.begin("a");
    let new = t.begin("a");
    assert!(!t.finish("a", old));
    assert!(t.finish("a", new));
    // Closed between two opens: only the open after the close counts.
    let first = t.begin("a");
    t.close("a");
    let second = t.begin("a");
    assert!(!t.finish("a", first));
    assert!(t.finish("a", second));
}

#[tokio::test]
async fn one_host_key_question_at_a_time() {
    let q = || ("hk-test.example".to_string(), 22u16, "SHA256:abc".to_string());
    let HostKeyTurn::Ask(asker) = host_key_turn(q()) else { panic!("first one asks") };
    let HostKeyTurn::Follow(rx) = host_key_turn(q()) else { panic!("second one follows") };
    // Another key on the same host is its own question.
    let HostKeyTurn::Ask(_other) = host_key_turn(("hk-test.example".into(), 22, "SHA256:xyz".into())) else {
        panic!("a different key asks for itself")
    };
    let waiting = tokio::spawn(asked_answer(rx));
    asker.answer(HostKeyAnswer::Accepted);
    assert_eq!(waiting.await.unwrap(), Some(HostKeyAnswer::Accepted));
    drop(asker);

    // Gone without an answer: the follower hears None and asks itself.
    let HostKeyTurn::Ask(asker) = host_key_turn(q()) else { panic!("asks again once the first is done") };
    let HostKeyTurn::Follow(rx) = host_key_turn(q()) else { panic!("follows") };
    let waiting = tokio::spawn(asked_answer(rx));
    drop(asker);
    assert_eq!(waiting.await.unwrap(), None);
    assert!(matches!(host_key_turn(q()), HostKeyTurn::Ask(_)));
}

// ── Speed ─────────────────────────────────────────────────────────────

/// How long a typed key's echo takes from `write_terminal_data`'s queue to
/// the batch handed to the page, over a real (in-process) SSH connection to
/// a server that echoes. A measure, not a check: it prints, and is run by
/// hand (`cargo test --lib measure_ -- --ignored --nocapture`). The pump's
/// flush window (8 ms from a batch's first byte) is most of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "a measure, run by hand"]
async fn measure_a_keys_echo_through_the_pump() {
    use crate::ssh_test_server::{connect, TestServer};
    use std::time::{Duration, Instant};

    let session = connect(TestServer::default(), |_| {}, crate::build_ssh_client_config()).await.expect("in-process SSH connection");
    let channel = session.channel_open_session().await.unwrap();
    channel.exec(false, "echo").await.unwrap();
    let (tx, rx) = mpsc::channel::<TerminalCommand>(32);
    let (_resize_tx, resize_rx) = tokio::sync::watch::channel(PtySize { cols: 80, rows: 24 });
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Instant>();
    let page = Arc::new(PageDrawn::default());
    let drew = Arc::clone(&page);
    let pump = tokio::spawn(pty_pump(channel, rx, resize_rx, page, Vec::new(), move |buf: &mut Vec<u8>| {
        drew.add(buf.len() as u64);
        buf.clear();
        let _ = out_tx.send(Instant::now());
    }));
    let mut took = Vec::new();
    for _ in 0..60 {
        // Typed a key at a time, as far apart as fast typing is.
        tokio::time::sleep(Duration::from_millis(60)).await;
        let at = Instant::now();
        tx.send(TerminalCommand::Data(vec![b'k'])).await.unwrap();
        let back = tokio::time::timeout(Duration::from_secs(5), out_rx.recv()).await.expect("the echo comes back").unwrap();
        took.push(back.duration_since(at).as_secs_f64() * 1000.0);
    }
    took.sort_by(f64::total_cmp);
    println!("a key's echo, queue to page: median {:.2} ms, fastest {:.2} ms, slowest {:.2} ms", took[took.len() / 2], took[0], took[took.len() - 1]);
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_secs(10), pump).await;
}

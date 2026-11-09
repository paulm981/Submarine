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

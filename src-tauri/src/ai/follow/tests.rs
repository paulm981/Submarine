//! follow, read_follow and stop_follow (#179), with a stand-in for the
//! server (`Fake`) and tokio's paused clock for the waits and time limits.

use super::*;
use std::sync::atomic::AtomicBool;

/// One follow the stand-in ran: what it was given, a way to end it as a
/// command would (an exit code), and whether it was stopped (where
/// exec::follow sends KILL and closes the channel).
struct Run {
    command: String,
    limit: Duration,
    feed: Arc<Feed>,
    exit: watch::Sender<Option<u32>>,
    killed: Arc<AtomicBool>,
}

#[derive(Default)]
struct Fake {
    runs: std::sync::Mutex<Vec<Run>>,
}

impl Fake {
    fn feed(&self, n: usize) -> Arc<Feed> {
        self.runs.lock().unwrap()[n].feed.clone()
    }
    fn killed(&self, n: usize) -> bool {
        self.runs.lock().unwrap()[n].killed.load(Ordering::SeqCst)
    }
    fn count(&self) -> usize {
        self.runs.lock().unwrap().len()
    }
    fn exit(&self, n: usize, code: u32) {
        let _ = self.runs.lock().unwrap()[n].exit.send(Some(code));
    }
}

/// A follower that runs nothing: it ends when stopped (recording the
/// KILL), at its limit, or when the test gives it an exit code.
fn fake_follower(fake: Arc<Fake>) -> Follower {
    Arc::new(move |_session, command, limit, mut stop, feed| {
        let (exit, mut exit_rx) = watch::channel(None);
        let killed = Arc::new(AtomicBool::new(false));
        fake.runs.lock().unwrap().push(Run { command, limit, feed, exit, killed: killed.clone() });
        Box::pin(async move {
            tokio::select! {
                _ = stop.wait_for(|s| *s) => {
                    killed.store(true, Ordering::SeqCst);
                    Ok(exec::Exec { stopped: true, ..exec::Exec::default() })
                }
                _ = tokio::time::sleep(limit) => {
                    killed.store(true, Ordering::SeqCst);
                    Ok(exec::Exec { timed_out: true, ..exec::Exec::default() })
                }
                _ = exit_rx.changed() => {
                    let code = *exit_rx.borrow();
                    Ok(exec::Exec { exit_status: code, ..exec::Exec::default() })
                }
            }
        })
    })
}

/// An AiState at `level` for every server, an AI tab `local-1` sharing
/// `sess-1` as `web1`, follows run by a Fake, and an audit log in a folder
/// of its own.
fn state(level: u8) -> (AiState, Arc<Fake>, PathBuf) {
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no commands in this test".to_string()) }));
    let dir = std::env::temp_dir().join(format!("submarine-follow-test-{}", channel::random_hex(6)));
    let s = AiState::with(exec, no_root(), Box::new(|| {}), Some(dir.clone()), Arc::new(move |_| trust::Policy { level, ..Default::default() }), Box::new(|| {}));
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    s.share_as("local-1", "sess-1", "web1", Some("deploy".into()), Some(7)).unwrap();
    let fake = Arc::new(Fake::default());
    s.set_follower(fake_follower(fake.clone()));
    (s, fake, dir)
}

/// A cancel signal that's never given.
fn no_cancel() -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    std::mem::forget(tx);
    rx
}

/// Start a follow that runs without asking; its id.
async fn start(s: &AiState, command: &str) -> String {
    let r = s.follow("local-1", "web1", command, no_cancel()).await;
    assert!(!r.is_error, "{}", r.text);
    id_in(&r.text)
}

fn id_in(text: &str) -> String {
    let at = text.find("Its id: ").expect("an id") + "Its id: ".len();
    text[at..at + 32].to_string()
}

async fn read(s: &AiState, id: &str) -> ToolResult {
    s.read_follow("local-1", id, no_cancel()).await
}

/// A read as the AI gets it: Submarine's lines before the tags, and what
/// the server sent between them.
fn parts(r: ToolResult) -> (String, String) {
    split(&r.for_ai().text)
}

fn split(t: &str) -> (String, String) {
    match t.find(boundary::DATA_LINE) {
        None => (t.to_string(), String::new()),
        Some(at) => {
            let rest = &t[at..];
            let open = rest.find(">\n").unwrap() + 2;
            let close = rest.rfind("</server-output id=\"").unwrap();
            assert!(rest.ends_with("\">"), "the closing tag ends it");
            (t[..at].to_string(), rest[open..close].to_string())
        }
    }
}

async fn until_ended(feed: &Feed) {
    while !feed.ended() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn audit_lines(dir: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join(audit::FILE_NAME)).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

// -- Asking ---------------------------------------------------------------------

/// A follow asks as run_command does, its card saying it keeps running; a
/// deny starts nothing, and is logged as a follow.
#[tokio::test(start_paused = true)]
async fn a_follow_asks_like_run_command_and_a_deny_starts_nothing() {
    let (s, fake, dir) = state(trust::ASK_EVERY_TIME);
    let s2 = s.clone();
    let req = tokio::spawn(async move { s2.follow("local-1", "web1", "journalctl -u nginx -f", no_cancel()).await });
    while s.snapshot().pending.is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let card = s.snapshot().pending[0].clone();
    assert!(card.follow && !card.as_root);
    assert_eq!(card.timeout_secs, LIFE.as_secs());
    assert_eq!(card.command, "journalctl -u nginx -f");
    s.decide(card.id, false).unwrap();
    let r = req.await.unwrap();
    assert!(r.is_error && r.text == "Denied by the user.", "{}", r.text);
    assert_eq!(fake.count(), 0, "nothing ran");
    assert!(s.snapshot().running.is_empty());
    let lines = audit_lines(&dir);
    let last = lines.last().unwrap();
    assert_eq!((last["request"].as_str(), last["decision"].as_str()), (Some("follow"), Some("denied")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Allowed on its card, it starts: in the running list as a follow, with
/// the whole life as its limit (not a command's), and its id goes to the AI.
#[tokio::test(start_paused = true)]
async fn allowed_it_starts_with_its_life_as_the_limit() {
    let (s, fake, dir) = state(trust::ASK_EVERY_TIME);
    let s2 = s.clone();
    let req = tokio::spawn(async move { s2.follow("local-1", "web1", "tail -f /var/log/syslog", no_cancel()).await });
    while s.snapshot().pending.is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    s.decide(s.snapshot().pending[0].id, true).unwrap();
    let r = req.await.unwrap();
    assert!(!r.is_error, "{}", r.text);
    assert_eq!(id_in(&r.text).len(), 32);
    assert_eq!(fake.count(), 1);
    {
        let runs = fake.runs.lock().unwrap();
        assert_eq!(runs[0].command, "tail -f /var/log/syslog");
        assert_eq!(runs[0].limit, LIFE);
    }
    fake.feed(0).push(b"12345");
    let running = s.snapshot().running;
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].follow.as_ref().map(|f| f.bytes_seen), Some(5));
    let _ = std::fs::remove_dir_all(&dir);
}

/// as_root and a timeout aren't offered: refused, nothing asked.
#[tokio::test(start_paused = true)]
async fn as_root_and_a_timeout_are_refused() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    for args in [
        serde_json::json!({ "server": "web1", "command": "tail -f x", "as_root": true }),
        serde_json::json!({ "server": "web1", "command": "tail -f x", "timeout_seconds": 60 }),
    ] {
        let r = s.follow_tool("local-1", "follow", &args, no_cancel()).await;
        assert!(r.is_error, "{args}");
    }
    assert_eq!(fake.count(), 0);
    assert!(s.snapshot().pending.is_empty());
    let r = s.follow_tool("local-1", "read_follow", &serde_json::json!({ "id": 5 }), no_cancel()).await;
    assert!(r.is_error && r.text.contains("id must be"), "{}", r.text);
    let _ = std::fs::remove_dir_all(&dir);
}

// -- Reading --------------------------------------------------------------------

/// Each read gives only what's new since the last, marked as the server's
/// after Submarine's own lines.
#[tokio::test(start_paused = true)]
async fn reads_give_only_what_is_new_inside_the_boundary() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    fake.feed(0).push(b"one\n");
    let (ours, printed) = parts(read(&s, &id).await);
    assert_eq!(printed, "one\n");
    assert_eq!(ours.lines().next(), Some("Still running."));
    assert!(!ours.contains("server-output"));
    fake.feed(0).push(b"two\nthree\n");
    let given = read(&s, &id).await.for_ai().text;
    // Submarine's words come first, outside the tags.
    assert!(given.find("Still running.").unwrap() < given.find("<server-output id=\"").unwrap());
    assert_eq!(split(&given).1, "two\nthree\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The ring keeps the newest RING bytes, from the start of a line, and the
/// next read says how much was dropped.
#[tokio::test(start_paused = true)]
async fn the_ring_drops_the_oldest_and_says_how_much() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let feed = fake.feed(0);
    // 400 lines of 1 KB: far more than the ring.
    let mut sent = Vec::new();
    for i in 0..400 {
        let line = format!("{i:04} {}\n", "x".repeat(1018));
        sent.extend_from_slice(line.as_bytes());
        feed.push(line.as_bytes());
    }
    let (ours, printed) = parts(read(&s, &id).await);
    let kept = RING / 1024 * 1024;
    let dropped = sent.len() - kept;
    assert!(ours.contains(&format!("{dropped} bytes were dropped since your last read (the buffer keeps the newest 256 KB).")), "{ours}");
    assert!(ours.contains("More is waiting: read again."), "{ours}");
    // Whole lines, the oldest kept first, at most a read's worth.
    assert!(printed.starts_with(&format!("{:04} ", 400 - kept / 1024)), "{}", &printed[..20]);
    assert_eq!(printed.len(), READ_MAX);
    // The rest, in order; nothing more dropped.
    let mut all = printed;
    loop {
        let (ours, printed) = parts(read(&s, &id).await);
        assert!(!ours.contains("dropped"));
        all.push_str(&printed);
        if !ours.contains("More is waiting") {
            break;
        }
    }
    assert_eq!(all.as_bytes(), &sent[dropped..]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_ring_wraps_and_drops_to_a_line_start() {
    let mut r = Ring::new();
    assert_eq!(r.push(&vec![b'a'; RING - 10]), 0);
    assert_eq!(r.take(RING - 20).len(), RING - 20);
    // Wraps round the end of its buffer.
    assert_eq!(r.push(b"0123456789\nabc\n"), 0);
    assert_eq!(&r.take(100)[..], b"aaaaaaaaaa0123456789\nabc\n");
    // A chunk bigger than all of it: the newest, from a line's start.
    let mut big = vec![b'z'; RING + 100];
    big[150] = b'\n';
    let dropped = r.push(&big);
    assert_eq!(dropped as usize, 100 + 51);
    assert_eq!(r.len, RING - 51);
    assert!(r.take(RING).iter().all(|c| *c == b'z'));
    // A line that never ends is kept as it is.
    let mut r = Ring::new();
    r.push(&vec![b'q'; RING]);
    assert_eq!(r.push(b"qq"), 2);
    assert_eq!(r.len, RING);
}

/// A read waits for a whole line and comes back as soon as one comes; with
/// nothing new it comes back after READ_WAIT.
#[tokio::test(start_paused = true)]
async fn a_read_waits_for_new_output() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let t = tokio::time::Instant::now();
    let (s2, id2) = (s.clone(), id.clone());
    let r = tokio::spawn(async move { read(&s2, &id2).await });
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!r.is_finished());
    fake.feed(0).push(b"arrived\n");
    let (_, printed) = parts(r.await.unwrap());
    assert_eq!(printed, "arrived\n");
    assert!(t.elapsed() < READ_WAIT);
    let t = tokio::time::Instant::now();
    let r = read(&s, &id).await;
    assert!(t.elapsed() >= READ_WAIT);
    assert!(r.server_text.is_none());
    assert_eq!(r.text, "Still running.\nNothing new.");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Only one read of a follow waits at a time.
#[tokio::test(start_paused = true)]
async fn a_second_read_while_one_waits_is_turned_down() {
    let (s, _fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let (s2, id2) = (s.clone(), id.clone());
    let first = tokio::spawn(async move { read(&s2, &id2).await });
    tokio::time::sleep(Duration::from_secs(1)).await;
    let r = read(&s, &id).await;
    assert!(r.is_error && r.text.contains("still waiting"), "{}", r.text);
    assert!(!first.await.unwrap().is_error);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A line still coming is held back, so a secret split between two pieces
/// of output is filtered whole; once nothing new comes for a whole wait,
/// what's held back is given.
#[tokio::test(start_paused = true)]
async fn a_partial_line_is_held_back_and_a_split_secret_still_hidden() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let feed = fake.feed(0);
    let token = format!("ghp_{}", "A1b2C3d4".repeat(5));
    feed.push(b"first\nlogin password=hun");
    let (_, one) = parts(read(&s, &id).await);
    assert_eq!(one, "first\n");
    feed.push(format!("ter2 and {}", &token[..10]).as_bytes());
    let (_, two) = parts(read(&s, &id).await);
    assert_eq!(two, "", "still no line end");
    feed.push(format!("{}\n", &token[10..]).as_bytes());
    let (ours, three) = parts(read(&s, &id).await);
    assert!(!three.contains("hun") && !three.contains("ter2") && !three.contains(&token[..10]) && !three.contains(&token[10..]), "{three}");
    assert_eq!(three, format!("login password={h} and {h}\n", h = filter::HIDDEN));
    assert!(ours.contains("Submarine hid 2 items"), "{ours}");
    // A prompt that never ends its line: held back once, then given after
    // a wait with nothing new.
    feed.push(b"Continue? ");
    let (_, held) = parts(read(&s, &id).await);
    assert_eq!(held, "");
    let (_, given) = parts(read(&s, &id).await);
    // (The closing tag goes on a line of its own.)
    assert_eq!(given, "Continue? \n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A private key whose block a read ends inside is held back until its END
/// line, then filtered whole.
#[tokio::test(start_paused = true)]
async fn a_private_key_split_across_reads_stays_hidden() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let feed = fake.feed(0);
    feed.push(b"before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAsecretpartone\n");
    let (ours, one) = parts(read(&s, &id).await);
    assert_eq!(one, "before\n");
    assert!(ours.contains("held back until it ends"), "{ours}");
    feed.push(b"BBBBsecretparttwo\n");
    let (_, two) = parts(read(&s, &id).await);
    assert_eq!(two, "");
    feed.push(b"CCCCsecretpartthree\n-----END OPENSSH PRIVATE KEY-----\nafter\n");
    let (_, three) = parts(read(&s, &id).await);
    assert_eq!(three, format!("{}\nafter\n", filter::HIDDEN));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The three kinds of block, each with its secret lines, in some output.
fn blocks() -> Vec<(&'static str, String, Vec<String>)> {
    use base64::Engine;
    let pem_body = ["b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW", "QyNTUxOQAAACBsecretpemlinetwoAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"];
    let pem = format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n{}\n-----END OPENSSH PRIVATE KEY-----\n", pem_body[0], pem_body[1]);
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("-----BEGIN RSA PRIVATE KEY-----\nMIIEsecretb64keymaterial{}\n-----END RSA PRIVATE KEY-----\n", "Q".repeat(120)));
    let wrapped: Vec<String> = encoded.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
    let b64 = format!("tls.key: {}\n", wrapped.join("\n"));
    let putty = "PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\nComment: me@host\nPublic-Lines: 1\nAAAAC3NzaC1lZDI1NTE5AAAAIpublicpart\nPrivate-Lines: 1\nAAAAIPrivatePartSECRETxyz0123456789\nPrivate-MAC: 0123abcd\n";
    vec![
        ("pem", pem, pem_body.iter().map(|l| l.to_string()).collect()),
        ("base64", b64, wrapped),
        ("putty", putty.to_string(), vec!["AAAAIPrivatePartSECRETxyz0123456789".to_string()]),
    ]
}

/// Each kind of block, split at every line and in the middle of every line
/// across two reads: none of its secret lines reach the AI, and the output
/// around it does.
#[tokio::test(start_paused = true)]
async fn a_key_block_split_anywhere_never_reaches_the_ai() {
    for (kind, block, secrets) in blocks() {
        let text = format!("before\n{block}after\n");
        let mut points: Vec<usize> = Vec::new();
        for (i, c) in text.char_indices() {
            if c == '\n' {
                points.push(i + 1);
            }
        }
        let starts: Vec<usize> = std::iter::once(0).chain(points.iter().copied()).collect();
        for w in starts.windows(2) {
            points.push((w[0] + w[1]) / 2);
        }
        // Right after the start marker, and inside the base64 marker.
        points.push(text.find("-----\n").map_or(10, |i| i + 6));
        points.push(text.find("LS0tLS1C").map_or(12, |i| i + 5));
        points.sort();
        points.dedup();
        points.retain(|p| *p > 0 && *p < text.len());
        for split in points {
            let (s, fake, dir) = state(trust::RUN_ANYTHING);
            let id = start(&s, "journalctl -f").await;
            let feed = fake.feed(0);
            let mut got = String::new();
            feed.push(&text.as_bytes()[..split]);
            got.push_str(&parts(read(&s, &id).await).1);
            got.push_str(&parts(read(&s, &id).await).1);
            feed.push(&text.as_bytes()[split..]);
            got.push_str(&parts(read(&s, &id).await).1);
            for secret in &secrets {
                assert!(!got.contains(secret.as_str()), "{kind} split at {split}: {secret} in {got:?}");
            }
            // (Each read's closing tag goes on a line of its own.)
            let joined = got.replace('\n', "");
            assert!(joined.starts_with("before") && joined.ends_with("after"), "{kind} split at {split}: {got:?}");
            assert!(got.contains(filter::HIDDEN), "{kind} split at {split}: {got:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// A block that doesn't end is held back up to the ring's size, then given
/// hidden, and so is the rest of it after; what follows it comes through.
#[tokio::test(start_paused = true)]
async fn an_unfinished_block_held_past_the_ring_comes_out_hidden() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let feed = fake.feed(0);
    feed.push(b"before\n-----BEGIN RSA PRIVATE KEY-----\n");
    let mut got = String::new();
    let mut hidden_said = false;
    // 400 KB of key lines, a read's worth at a time: past the ring before
    // its END line comes.
    for _ in 0..400 {
        feed.push(format!("{}\n", "K".repeat(1023)).as_bytes());
        if feed.lock().ring.len >= READ_MAX {
            let (ours, printed) = parts(read(&s, &id).await);
            hidden_said |= ours.contains("Submarine hid");
            got.push_str(&printed);
        }
    }
    feed.push(b"KKKK\n-----END RSA PRIVATE KEY-----\nafter\n");
    loop {
        let (ours, printed) = parts(read(&s, &id).await);
        got.push_str(&printed);
        if !ours.contains("More is waiting") {
            break;
        }
    }
    assert!(hidden_said, "hidden before its end came");
    assert!(!got.contains("KKKK"), "{}", &got[..got.len().min(300)]);
    assert!(got.starts_with("before\n") && got.ends_with("after\n"), "{got:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A block still held when the follow ends comes out hidden.
#[tokio::test(start_paused = true)]
async fn a_block_held_when_the_follow_ends_comes_out_hidden() {
    for block in ["-----BEGIN EC PRIVATE KEY-----\nSECRETline1\nSECRETline2", "key: LS0tLS1CRUdJTiBSU0EgUFJJVkFURSBLRVktLS0tLQ\nU0VDUkVUc2VjcmV0U0VDUkVU\n", "PuTTY-User-Key-File-3: ssh-rsa\nPrivate-Lines: 1\nSECRETprivate\n"] {
        let (s, fake, dir) = state(trust::RUN_ANYTHING);
        let id = start(&s, "journalctl -f").await;
        let feed = fake.feed(0);
        feed.push(format!("before\n{block}").as_bytes());
        let (_, one) = parts(read(&s, &id).await);
        assert_eq!(one, "before\n", "{block}");
        fake.exit(0, 0);
        until_ended(&feed).await;
        let (ours, last) = parts(read(&s, &id).await);
        assert!(ours.starts_with("Ended: exit code 0."), "{ours}");
        assert_eq!(last.trim_end(), filter::HIDDEN, "{block}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A line still coming that may be a block's first line isn't given after a
/// quiet wait, as a prompt is: a marker split there would let the block by.
#[tokio::test(start_paused = true)]
async fn a_line_that_may_start_a_block_waits_for_its_end() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let feed = fake.feed(0);
    feed.push(b"-----BEGIN OPENSSH PRIV");
    assert_eq!(parts(read(&s, &id).await).1, "");
    assert_eq!(parts(read(&s, &id).await).1, "");
    feed.push(b"ATE KEY-----\nSECRETbody\n-----END OPENSSH PRIVATE KEY-----\nok\n");
    let got = parts(read(&s, &id).await).1;
    assert_eq!(got, format!("{}\nok\n", filter::HIDDEN));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hints_of_a_block_start() {
    for yes in [&b"x -----BEGIN"[..], b"abc --", b"key: LS0", b"PuT", b"--"] {
        assert!(may_start_block(yes), "{}", String::from_utf8_lossy(yes));
    }
    for no in [&b"Continue? "[..], b"progress 42%", b"a - b"] {
        assert!(!may_start_block(no), "{}", String::from_utf8_lossy(no));
    }
}

// -- Limits and ids -------------------------------------------------------------

/// At most MAX_FOLLOWS per AI tab, an ended one not read to the end
/// counting; reading it to the end makes room.
#[tokio::test(start_paused = true)]
async fn at_most_four_follows_per_ai_tab() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let mut ids = Vec::new();
    for i in 0..MAX_FOLLOWS {
        ids.push(start(&s, &format!("tail -f /var/log/{i}")).await);
    }
    let r = s.follow("local-1", "web1", "tail -f /var/log/x", no_cancel()).await;
    assert!(r.is_error && r.text.contains("4 follows are already open"), "{}", r.text);
    // One ends by itself: still open until it's read to the end.
    fake.exit(0, 0);
    until_ended(&fake.feed(0)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(s.follow("local-1", "web1", "tail -f /var/log/x", no_cancel()).await.is_error);
    let (ours, _) = parts(read(&s, &ids[0]).await);
    assert!(ours.starts_with("Ended: exit code 0."), "{ours}");
    start(&s, "tail -f /var/log/x").await;
    // Commands still run beside them: follows aren't counted as commands.
    assert_eq!(running_on(&s.lock(), "local-1", "sess-1"), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A follow's id works only in its own AI tab.
#[tokio::test(start_paused = true)]
async fn another_ai_tab_can_t_read_or_stop_it() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    s.start_tab("local-2", "Codex").unwrap();
    s.share_as("local-2", "sess-1", "web1", None, Some(7)).unwrap();
    fake.feed(0).push(b"mine\n");
    let r = s.read_follow("local-2", &id, no_cancel()).await;
    assert!(r.is_error && r.text.starts_with("No follow with that id"), "{}", r.text);
    let r = s.stop_follow("local-2", &id).await;
    assert!(r.is_error);
    assert!(!fake.killed(0));
    let (_, printed) = parts(read(&s, &id).await);
    assert_eq!(printed, "mine\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// -- Ending ---------------------------------------------------------------------

/// stop_follow ends it on the server (the stand-in's KILL), gives what's
/// left and how it ended, and the id is gone; the log has one line as it
/// started and one as it ended.
#[tokio::test(start_paused = true)]
async fn stop_follow_ends_it_and_the_log_has_its_start_and_end() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -u nginx -f").await;
    fake.feed(0).push(b"last words\n");
    let (ours, printed) = parts(s.stop_follow("local-1", &id).await);
    assert!(fake.killed(0));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(ours.starts_with("Ended: stopped by you (stop_follow). That was the last of it"), "{ours}");
    assert_eq!(printed, "last words\n");
    let r = read(&s, &id).await;
    assert!(r.is_error && r.text.starts_with("No follow with that id"), "{}", r.text);
    assert!(s.snapshot().running.is_empty());
    let h = &s.snapshot().history[0];
    assert_eq!(h.outcome, "stopped");
    let lines: Vec<Value> = audit_lines(&dir).into_iter().filter(|l| l["request"] == "follow").collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!((lines[0]["event"].as_str(), lines[0]["decision"].as_str()), (Some("request"), Some("allowed")));
    assert_eq!(lines[1]["event"], "finished");
    assert_eq!(lines[0]["id"], lines[1]["id"]);
    assert!(lines[1]["note"].as_str().unwrap().contains("stopped by the AI"), "{}", lines[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// After it ends by itself, the last read gives the rest and how it ended,
/// and the id is gone.
#[tokio::test(start_paused = true)]
async fn the_last_read_gives_the_rest_and_how_it_ended() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    fake.feed(0).push(b"a\nb\nno line end");
    fake.exit(0, 3);
    until_ended(&fake.feed(0)).await;
    let (ours, printed) = parts(read(&s, &id).await);
    assert!(ours.starts_with("Ended: exit code 3. That was the last of it"), "{ours}");
    assert_eq!(printed, "a\nb\nno line end\n");
    assert!(!fake.killed(0), "it ended by itself");
    assert!(read(&s, &id).await.text.starts_with("No follow with that id"));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(s.snapshot().history[0].exit_code, Some(3));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Stop in the AI panel ends it, and the AI hears who stopped it.
#[tokio::test(start_paused = true)]
async fn stop_in_the_panel_ends_it() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    let request = s.snapshot().running[0].id;
    s.stop_follow_by_you(request).unwrap();
    until_ended(&fake.feed(0)).await;
    assert!(fake.killed(0));
    let (ours, _) = parts(read(&s, &id).await);
    assert!(ours.starts_with("Ended: stopped by the user in Submarine."), "{ours}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(s.stop_follow_by_you(request).is_err(), "not running any more");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Nobody reading it for UNREAD stops it; reading keeps it going.
#[tokio::test(start_paused = true)]
async fn thirty_minutes_unread_stops_it() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    tokio::time::sleep(UNREAD - Duration::from_secs(60)).await;
    read(&s, &id).await;
    tokio::time::sleep(UNREAD - Duration::from_secs(60)).await;
    assert!(!fake.killed(0), "a read kept it going");
    tokio::time::sleep(Duration::from_secs(120)).await;
    until_ended(&fake.feed(0)).await;
    assert!(fake.killed(0));
    let (ours, _) = parts(read(&s, &id).await);
    assert!(ours.starts_with("Ended: stopped after 30 minutes with no reads."), "{ours}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// However often it's read, it ends after LIFE.
#[tokio::test(start_paused = true)]
async fn it_ends_after_two_hours_however_often_it_is_read() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_secs(25 * 60)).await;
        if fake.feed(0).ended() {
            break;
        }
        read(&s, &id).await;
    }
    until_ended(&fake.feed(0)).await;
    let (ours, _) = parts(read(&s, &id).await);
    assert!(ours.starts_with("Ended: stopped after 2 hours, the most a follow runs."), "{ours}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(s.snapshot().history[0].timed_out);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Unsharing the server, closing the AI tab and turning AI tools off each
/// end it on the server.
#[tokio::test(start_paused = true)]
async fn an_end_of_access_ends_it() {
    for how in ["unshare", "tab", "off"] {
        let (s, fake, dir) = state(trust::RUN_ANYTHING);
        let id = start(&s, "journalctl -f").await;
        match how {
            "unshare" => s.session_gone("sess-1", "closed"),
            "tab" => s.end_tab("local-1"),
            _ => s.turn_off().await,
        }
        until_ended(&fake.feed(0)).await;
        assert!(fake.killed(0), "{how}");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(s.snapshot().running.is_empty(), "{how}");
        let r = read(&s, &id).await;
        if how == "unshare" {
            // The AI tab is still there: its last read says why.
            let (ours, _) = parts(r);
            assert!(ours.starts_with("Ended: stopped: the server isn't shared any more."), "{ours}");
        } else {
            assert!(r.is_error, "{how}: {}", r.text);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Withdrawn by the AI while it waits for you, it never starts.
#[tokio::test(start_paused = true)]
async fn withdrawn_while_it_waits_it_never_starts() {
    let (s, fake, dir) = state(trust::ASK_EVERY_TIME);
    let (cancel_tx, cancel) = watch::channel(false);
    let s2 = s.clone();
    let req = tokio::spawn(async move { s2.follow("local-1", "web1", "journalctl -f", cancel).await });
    while s.snapshot().pending.is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancel_tx.send(true).unwrap();
    assert!(req.await.unwrap().is_error);
    assert_eq!(fake.count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An ended follow nobody reads to the end goes UNREAD after it ended:
/// its slot is free again, and the log says so once.
#[tokio::test(start_paused = true)]
async fn an_ended_follow_nobody_reads_is_removed() {
    let (s, fake, dir) = state(trust::RUN_ANYTHING);
    let id = start(&s, "journalctl -f").await;
    fake.feed(0).push(b"never read\n");
    tokio::time::sleep(Duration::from_secs(10 * 60)).await;
    fake.exit(0, 0);
    until_ended(&fake.feed(0)).await;
    // Still there just short of UNREAD after it ended, though it last
    // started a read longer ago than that.
    tokio::time::sleep(UNREAD - Duration::from_secs(60)).await;
    assert!(s.follow_of("local-1", &id).is_some());
    assert_eq!(open_in(&s.lock(), "local-1"), 1);
    tokio::time::sleep(Duration::from_secs(120)).await;
    assert!(s.follow_of("local-1", &id).is_none());
    assert_eq!(open_in(&s.lock(), "local-1"), 0);
    assert!(read(&s, &id).await.text.starts_with("No follow with that id"));
    let removed: Vec<Value> = audit_lines(&dir).into_iter().filter(|l| l["event"] == "follow_removed").collect();
    assert_eq!(removed.len(), 1, "{removed:?}");
    assert!(removed[0]["note"].as_str().unwrap().contains("not read to the end"));
    let _ = std::fs::remove_dir_all(&dir);
}

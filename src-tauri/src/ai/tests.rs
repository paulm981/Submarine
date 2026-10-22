//! The bridge end to end: a helper (helper::relay, with in-memory stdin and
//! stdout) over the real local channel to an AiState, and run_command
//! against the test SSH server.

use super::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// An AiState whose commands fail (no servers), turned on.
fn state_without_servers() -> AiState {
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), None);
    s.turn_on().unwrap();
    s
}

/// A helper for `env` (the variables start_tab gave), with stdin and stdout
/// to drive it by.
struct Helper {
    to: tokio::io::DuplexStream,
    from: BufReader<tokio::io::DuplexStream>,
    task: tokio::task::JoinHandle<Result<(), String>>,
}

fn var<'a>(env: &'a [(String, String)], k: &str) -> &'a str {
    &env.iter().find(|(n, _)| n == k).unwrap().1
}

fn helper(addr: &str, token: &str) -> Helper {
    let (to, stdin) = tokio::io::duplex(1 << 20);
    let (stdout, from) = tokio::io::duplex(1 << 20);
    let (addr, token) = (addr.to_string(), token.to_string());
    let task = tokio::spawn(async move { helper::relay(stdin, stdout, &addr, &token, Some(std::process::id())).await });
    Helper { to, from: BufReader::new(from), task }
}

impl Helper {
    async fn send(&mut self, msg: Value) {
        let mut line = msg.to_string();
        line.push('\n');
        self.to.write_all(line.as_bytes()).await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(20), self.from.read_line(&mut line)).await.expect("an answer").unwrap();
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON: {line:?}"))
    }

    async fn call(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await;
        let r = self.recv().await;
        assert_eq!(r["id"], id);
        r
    }
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "waited for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// An AI tool's SSH_AUTH_SOCK names a place with no agent: set, not
/// unset, so Windows OpenSSH doesn't fall back to its own agent's pipe.
#[tokio::test]
async fn an_ai_tool_is_pointed_at_no_ssh_agent() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let sock = var(&env, "SSH_AUTH_SOCK");
    #[cfg(windows)]
    {
        // A name no one could have made ahead of it: new each AI tab.
        let suffix = sock.strip_prefix(r"\\.\pipe\submarine-no-ssh-agent-").expect(sock);
        assert!(suffix.len() == 16 && suffix.bytes().all(|b| b.is_ascii_hexdigit()), "{sock}");
        let (_, env2) = s.start_tab("local-2", "Claude").unwrap();
        assert_ne!(var(&env2, "SSH_AUTH_SOCK"), sock);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        assert_eq!(sock, "/dev/null");
        assert!(!std::fs::metadata(sock).unwrap().file_type().is_socket());
    }
    assert!(!CREDENTIAL_ENV.contains(&"SSH_AUTH_SOCK"));
}

#[tokio::test]
async fn the_helper_talks_mcp_over_stdin_and_stdout() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let mut h = helper(var(&env, channel::ENV_ADDR), var(&env, channel::ENV_TOKEN));
    let init = h.call(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } })).await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "submarine");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    h.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    let tools = h.call(2, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["list_shared", "run_command"]);
    // MCP's standard hints.
    let listed_tools = &tools["result"]["tools"];
    assert_eq!(listed_tools[0]["annotations"], json!({ "readOnlyHint": true }));
    assert_eq!(listed_tools[1]["annotations"], json!({ "destructiveHint": true, "openWorldHint": true }));
    let listed = h.call(3, "tools/call", json!({ "name": "list_shared", "arguments": {} })).await;
    assert_eq!(listed["result"]["isError"], false);
    assert!(listed["result"]["content"][0]["text"].as_str().unwrap().contains("No servers are shared"));
    // An unknown version gets the newest; an unknown method an error.
    let init = h.call(4, "initialize", json!({ "protocolVersion": "1999-01-01" })).await;
    assert_eq!(init["result"]["protocolVersion"], mcp_newest());
    assert_eq!(h.call(5, "resources/list", json!({})).await["error"]["code"], -32601);
    // Closing stdin ends the helper.
    drop(h.to);
    assert!(tokio::time::timeout(Duration::from_secs(5), h.task).await.expect("the helper ended").unwrap().is_ok());
}

fn mcp_newest() -> &'static str {
    "2025-11-25"
}

#[tokio::test]
async fn the_channel_refuses_a_wrong_token() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let addr = var(&env, channel::ENV_ADDR).to_string();
    let wrong = channel::random_hex(32);
    let err = helper::relay(tokio::io::empty(), tokio::io::sink(), &addr, &wrong, None).await.unwrap_err();
    assert!(err.contains("doesn't know this AI tab"), "{err}");
    // Nor an empty one, or garbage instead of a hello.
    assert!(helper::relay(tokio::io::empty(), tokio::io::sink(), &addr, "", None).await.is_err());
    let mut raw = BufReader::new(channel::connect(&addr).await.unwrap());
    raw.get_mut().write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    let answer = channel::read_line(&mut raw, 4096).await.unwrap().unwrap();
    assert!(answer.contains("\"ok\":false"), "{answer}");
}

#[tokio::test]
async fn a_closed_ai_tab_s_token_stops_working_and_its_connection_closes() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let (addr, token) = (var(&env, channel::ENV_ADDR).to_string(), var(&env, channel::ENV_TOKEN).to_string());
    let mut h = helper(&addr, &token);
    h.call(1, "ping", json!({})).await;
    s.end_tab("local-1");
    // The open connection ends...
    assert!(tokio::time::timeout(Duration::from_secs(5), h.task).await.expect("the helper ended").unwrap().is_ok());
    // ...and the token can't connect again.
    assert!(helper::relay(tokio::io::empty(), tokio::io::sink(), &addr, &token, None).await.is_err());
}

/// One live connection per AI tab: a second with the same token, while the
/// first is open, is refused and the panel warns; the first carries on.
#[tokio::test]
async fn a_second_connection_with_a_tab_s_token_is_refused() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let (addr, token) = (var(&env, channel::ENV_ADDR).to_string(), var(&env, channel::ENV_TOKEN).to_string());
    let mut first = helper(&addr, &token);
    first.call(1, "ping", json!({})).await;
    assert!(s.snapshot().tabs[0].intruder_at.is_none());
    let t = Instant::now();
    let err = helper::relay(tokio::io::empty(), tokio::io::sink(), &addr, &token, None).await.unwrap_err();
    assert!(err.contains("already connected"), "{err}");
    assert!(t.elapsed() >= RECONNECT_GRACE - Duration::from_millis(100), "waited for the first to end");
    assert!(s.snapshot().tabs[0].intruder_at.is_some(), "the panel warns");
    first.call(2, "ping", json!({})).await;
}

/// The AI tool reconnecting (its old helper ends, a new one starts) gets
/// through: once the first has ended, and also when the new one comes
/// before the app has seen the old one close.
#[tokio::test]
async fn a_reconnect_after_the_first_helper_ends_gets_through() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let (addr, token) = (var(&env, channel::ENV_ADDR).to_string(), var(&env, channel::ENV_TOKEN).to_string());
    let mut first = helper(&addr, &token);
    first.call(1, "ping", json!({})).await;
    drop(first.to);
    assert!(tokio::time::timeout(Duration::from_secs(5), first.task).await.expect("ended").unwrap().is_ok());
    let mut second = helper(&addr, &token);
    second.call(1, "ping", json!({})).await;
    // At once, without waiting for it to end.
    drop(second.to);
    let mut third = helper(&addr, &token);
    third.call(1, "ping", json!({})).await;
    assert!(s.snapshot().tabs[0].intruder_at.is_none(), "no warning for a reconnect");
}

/// The helper sends nothing to a channel whose other end isn't the app's
/// process (another program listening in its place).
#[tokio::test]
async fn the_helper_wont_talk_to_another_process() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let (addr, token) = (var(&env, channel::ENV_ADDR).to_string(), var(&env, channel::ENV_TOKEN).to_string());
    assert_eq!(var(&env, channel::ENV_PID), std::process::id().to_string());
    let err = helper::relay(tokio::io::empty(), tokio::io::sink(), &addr, &token, Some(std::process::id() + 1)).await.unwrap_err();
    assert!(err.contains("taken the place"), "{err}");
    // Nothing reached the app: the token was never used.
    let mut h = helper(&addr, &token);
    h.call(1, "ping", json!({})).await;
    assert!(s.snapshot().tabs[0].intruder_at.is_none());
}

#[tokio::test]
async fn turning_ai_tools_off_ends_access_and_listening() {
    let s = state_without_servers();
    let (_, env) = s.start_tab("local-1", "Claude").unwrap();
    let addr = var(&env, channel::ENV_ADDR).to_string();
    s.turn_off().await;
    assert!(s.snapshot().tabs.is_empty());
    assert!(channel::connect(&addr).await.is_err(), "nothing listens");
    assert!(s.start_tab("local-2", "Claude").is_err(), "no AI tab starts while it's off");
}

#[tokio::test]
async fn each_ai_tab_only_reaches_what_was_shared_with_it() {
    let s = state_without_servers();
    s.start_tab("local-a", "Claude").unwrap();
    let (label_b, _) = s.start_tab("local-b", "Claude").unwrap();
    assert_eq!(label_b, "Claude 2");
    s.share("local-a", "sess-1", "xyz").unwrap();
    let (_, no_cancel) = watch::channel(false);
    let r = s.run_command("local-b", "xyz", "uptime", None, no_cancel.clone()).await;
    assert!(r.is_error && r.text.contains("isn't shared with you"), "{}", r.text);
    assert!(s.snapshot().pending.is_empty(), "never shown to you");
    assert!(s.list_shared("local-b").text.contains("No servers"));
    assert!(s.list_shared("local-a").text.contains("xyz"));
}

#[test]
fn ai_tools_start_in_a_folder_of_their_own() {
    let data = std::env::temp_dir().join(format!("submarine-ai-workspace-{}", std::process::id()));
    let dir = workspace(&data).unwrap();
    assert_eq!(dir, data.join("ai-workspace"));
    assert!(dir.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |d: &std::path::Path| std::fs::metadata(d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        // Made otherwise, it's made private again.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(workspace(&data).unwrap(), dir);
        assert_eq!(mode(&dir), 0o700);
        // A link isn't followed.
        std::fs::remove_dir(&dir).unwrap();
        std::os::unix::fs::symlink(std::env::temp_dir(), &dir).unwrap();
        assert!(workspace(&data).is_err());
    }
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn names_stay_unique_within_an_ai_tab() {
    let mut servers = Vec::new();
    for _ in 0..3 {
        let name = unique_name(&servers, "web");
        servers.push(SharedServer { session_id: name.clone(), name, user: None });
    }
    let names: Vec<&str> = servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["web", "web (2)", "web (3)"]);
}

#[test]
fn root_words_are_seen() {
    for cmd in ["sudo cat /etc/shadow", "ls; sudo -n true", "/usr/bin/sudo x", "echo hi | doas tee f", "(su -)", "dzdo ls", "pkexec id", "x && su"] {
        assert!(asks_for_root(cmd), "{cmd}");
    }
    for cmd in ["ls -su", "cat sudoers.txt", "issue", "echo pseudo", "visudo-ish"] {
        assert!(!asks_for_root(cmd), "{cmd}");
    }
}

#[test]
fn long_output_is_shortened_for_the_ai() {
    let short = "x".repeat(100);
    assert_eq!(shorten_for_ai(&short), short);
    let long = format!("{}{}{}", "a".repeat(AI_HEAD), "m".repeat(5000), "z".repeat(AI_TAIL));
    let s = shorten_for_ai(&long);
    assert!(s.starts_with("aaa") && s.ends_with("zzz") && s.contains("[5000 bytes left out by Submarine]"));
}

#[tokio::test]
async fn requests_are_limited() {
    let s = state_without_servers();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    let (_cancel_tx, cancel) = watch::channel(false);
    // Three may wait at once; the fourth is refused.
    let mut waiting = Vec::new();
    for _ in 0..MAX_WAITING {
        let (s2, c) = (s.clone(), cancel.clone());
        waiting.push(tokio::spawn(async move { s2.run_command("local-1", "xyz", "uptime", None, c).await }));
    }
    wait_for("the requests", || s.snapshot().pending.len() == MAX_WAITING).await;
    let r = s.run_command("local-1", "xyz", "uptime", None, cancel.clone()).await;
    assert!(r.is_error && r.text.contains("already waiting"), "{}", r.text);
    // Bad arguments never reach you.
    for (cmd, t) in [("", None), ("x\0y", None), ("uptime", Some(0)), ("uptime", Some(MAX_TIMEOUT + 1))] {
        let r = s.run_command("local-1", "xyz", cmd, t, cancel.clone()).await;
        assert!(r.is_error, "{cmd:?} {t:?}");
    }
    assert_eq!(s.snapshot().pending.len(), MAX_WAITING);
    // Denying frees them.
    for p in s.snapshot().pending {
        s.decide(p.id, false).unwrap();
    }
    for w in waiting {
        let r = w.await.unwrap();
        assert!(r.is_error && r.text == "Denied by the user.");
    }
}

#[tokio::test]
async fn unsharing_cancels_what_waits() {
    let s = state_without_servers();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    let (_cancel_tx, cancel) = watch::channel(false);
    let s2 = s.clone();
    let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "uptime", None, cancel).await });
    wait_for("the request", || s.snapshot().pending.len() == 1).await;
    // The server's tab reconnects: sharing stops.
    s.session_gone("sess-1", "reconnected");
    let r = req.await.unwrap();
    assert!(r.is_error && r.text.starts_with("Cancelled"), "{}", r.text);
    assert!(s.snapshot().tabs[0].servers.is_empty());
    assert!(s.snapshot().pending.is_empty());
    assert_eq!(s.snapshot().history[0].outcome, "cancelled");
}

#[tokio::test]
async fn the_ai_withdrawing_a_request_cancels_it() {
    let s = state_without_servers();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    let (cancel_tx, cancel) = watch::channel(false);
    let s2 = s.clone();
    let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "uptime", None, cancel).await });
    wait_for("the request", || s.snapshot().pending.len() == 1).await;
    let id = s.snapshot().pending[0].id;
    cancel_tx.send(true).unwrap();
    assert!(req.await.unwrap().is_error);
    assert!(s.snapshot().pending.is_empty());
    assert!(s.decide(id, true).is_err(), "nothing left to allow");
}

/// An AiState whose commands all print `output`, turned on, with one AI
/// tab sharing `sess-1` as `xyz`.
fn state_printing(output: &'static str) -> AiState {
    let exec: Executor = Arc::new(move |_, _, _, _| {
        Box::pin(async move { Ok(exec::Exec { output: output.as_bytes().to_vec(), exit_status: Some(0), ..exec::Exec::default() }) })
    });
    let s = AiState::new(exec, Box::new(|| {}), None);
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    s
}

/// Run `command` on `xyz` and allow it.
async fn run_allowed(s: &AiState, command: &str) -> mcp::ToolResult {
    let (_c, cancel) = watch::channel(false);
    let (s2, cmd) = (s.clone(), command.to_string());
    let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", &cmd, None, cancel).await });
    wait_for("the request", || !s.snapshot().pending.is_empty()).await;
    s.decide(s.snapshot().pending[0].id, true).unwrap();
    req.await.unwrap()
}

/// Locking the profile (close_profile ends every AI tab) or turning AI
/// tools off empties the panel's history, outputs and all; a request that
/// was still waiting doesn't bring its row back once it's cancelled.
#[tokio::test]
async fn locking_or_turning_off_forgets_what_ran() {
    for off in [false, true] {
        let s = state_printing("db password is hunter2");
        assert!(!run_allowed(&s, "cat notes").await.is_error);
        assert_eq!(s.snapshot().history.len(), 1);
        let id = s.snapshot().history[0].id;
        assert!(s.lock().history[0].output.contains("hunter2"));
        // One more, left waiting.
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let waiting = tokio::spawn(async move { s2.run_command("local-1", "xyz", "uptime", None, cancel).await });
        wait_for("the request", || s.snapshot().pending.len() == 1).await;
        if off {
            s.turn_off().await;
        } else {
            s.end_all_tabs();
        }
        assert!(waiting.await.unwrap().is_error);
        assert!(s.snapshot().history.is_empty(), "off: {off}");
        assert!(s.lock().history.iter().all(|d| d.view.id != id));
        // A new AI tab starts with an empty history, and fills it again.
        if off {
            s.turn_on().unwrap();
        }
        s.start_tab("local-2", "Claude").unwrap();
        assert!(s.snapshot().history.is_empty());
    }
}

/// A request asked while every AI tab's access is ending (here, from the
/// page's change notice, between one tab ending and the next) leaves no
/// row in the cleared history: the tabs and the history go in one step.
#[tokio::test]
async fn a_request_asked_while_access_ends_leaves_no_row() {
    use futures::FutureExt;
    use std::sync::{Mutex as StdMutex, OnceLock};
    type Asked = std::pin::Pin<Box<dyn std::future::Future<Output = mcp::ToolResult> + Send>>;

    let state: Arc<OnceLock<AiState>> = Arc::new(OnceLock::new());
    let asked: Arc<StdMutex<Option<Asked>>> = Arc::new(StdMutex::new(None));
    let (_cancel_tx, cancel) = watch::channel(false);
    let ending = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let on_change = {
        let (state, asked, ending) = (Arc::clone(&state), Arc::clone(&asked), Arc::clone(&ending));
        Box::new(move || {
            // Once, at the first notice while access ends (not again from
            // inside the request's own notice).
            if !ending.swap(false, Ordering::SeqCst) {
                return;
            }
            let Some(s) = state.get() else { return };
            // Asked by whichever tab is still there, run up to where it
            // waits for you.
            let Some(tab) = s.lock().tabs.first().map(|t| t.id.clone()) else { return };
            let (s, cancel) = (s.clone(), cancel.clone());
            let server = if tab == "local-1" { "xyz" } else { "abc" };
            let mut f: Asked = Box::pin(async move { s.run_command(&tab, server, "uptime", None, cancel).await });
            if let Some(r) = (&mut f).now_or_never() {
                assert!(r.is_error, "refused at once: {}", r.text);
            }
            *asked.lock().unwrap() = Some(f);
        }) as Box<dyn Fn() + Send + Sync>
    };
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, on_change, None);
    assert!(state.set(s.clone()).is_ok());
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    s.start_tab("local-2", "Claude").unwrap();
    s.share("local-2", "sess-2", "abc").unwrap();

    ending.store(true, Ordering::SeqCst);
    s.end_all_tabs();
    let f = asked.lock().unwrap().take();
    if let Some(f) = f {
        assert!(f.await.is_error);
    }
    assert!(s.snapshot().tabs.is_empty());
    assert!(s.snapshot().pending.is_empty());
    assert!(s.snapshot().history.is_empty(), "{:?}", s.snapshot().history.iter().map(|d| d.outcome).collect::<Vec<_>>());
}

/// The AI can't flush the audit log: every call counts toward the rate
/// limit, refused or not; past it one line is logged a minute, not one a
/// call; and a refused command is logged cut short.
#[tokio::test]
async fn refusals_count_toward_the_limit_and_are_logged_cut_short() {
    let dir = std::env::temp_dir().join(format!("submarine-ai-test-{}", channel::random_hex(6)));
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), Some(dir.clone()));
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    let (_c, cancel) = watch::channel(false);
    let huge = "x".repeat(200 * 1024);
    for _ in 0..RATE_MAX + 30 {
        let r = s.run_command("local-1", "nowhere", &huge, None, cancel.clone()).await;
        assert!(r.is_error);
    }
    let r = s.run_command("local-1", "nowhere", "uptime", None, cancel.clone()).await;
    assert!(r.text.contains("Too many requests"), "{}", r.text);
    let text = std::fs::read_to_string(dir.join(audit::FILE_NAME)).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let refused: Vec<&Value> = lines.iter().filter(|l| l["decision"] == "refused").collect();
    assert_eq!(refused.len(), RATE_MAX + 1, "{RATE_MAX} refusals, then one line for the limit");
    assert!(refused.last().unwrap()["note"].as_str().unwrap().contains("Too many requests"));
    for l in &refused {
        assert!(l["command"].as_str().unwrap().len() <= MAX_LOGGED_COMMAND + 64);
    }
    assert!(text.len() < (RATE_MAX + 2) * (MAX_LOGGED_COMMAND + 1024), "{} bytes", text.len());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server name of a megabyte, sent again and again, can't fill the log:
/// the name is cut in the line, and in the note that says why it was
/// refused.
#[tokio::test]
async fn long_server_names_are_logged_cut_short() {
    let dir = std::env::temp_dir().join(format!("submarine-ai-test-{}", channel::random_hex(6)));
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), Some(dir.clone()));
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    let (_c, cancel) = watch::channel(false);
    let huge = "\"".repeat(1024 * 1024);
    for shared in [false, true] {
        if shared {
            s.share("local-1", "sess-1", "xyz").unwrap();
        }
        let r = s.run_command("local-1", &huge, "uptime", None, cancel.clone()).await;
        assert!(r.is_error && r.text.contains("isn't shared with you"), "{}", &r.text[..200.min(r.text.len())]);
        assert!(r.text.len() < 4 * 1024, "the AI's answer is cut too: {} bytes", r.text.len());
    }
    let text = std::fs::read_to_string(dir.join(audit::FILE_NAME)).unwrap();
    let refused: Vec<&str> = text.lines().filter(|l| l.contains("\"refused\"")).collect();
    assert_eq!(refused.len(), 2);
    for l in refused {
        assert!(l.len() < MAX_LOGGED_NAME + MAX_LOGGED_NOTE + 1024, "{} bytes", l.len());
        assert!(l.contains("sha256 "));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn commands_are_logged_cut_short() {
    assert_eq!(for_log("abc", 10), "abc");
    // Whole characters, and the hash of all of it.
    let cut = for_log("ééé", 3);
    assert!(cut.starts_with("é… [4 bytes more; sha256 "), "{cut}");
    assert_eq!(cut.len(), "é… [4 bytes more; sha256 ]".len() + 16);
    assert_ne!(for_log("éééx", 3), cut, "different commands, different hashes");
    // Escapes count: what's written stays within the limit.
    for nasty in ["\u{1}".repeat(10_000), "\"".repeat(10_000), "\\".repeat(10_000), "é".repeat(10_000)] {
        let logged = for_log(&nasty, MAX_LOGGED_COMMAND);
        let json = serde_json::to_string(&logged).unwrap();
        assert!(json.len() <= MAX_LOGGED_COMMAND + 80, "{}", json.len());
    }
}

/// Asking and withdrawing, without you allowing anything, can't fill the
/// log: each request's command is logged cut short.
#[tokio::test]
async fn withdrawn_requests_are_logged_cut_short() {
    let dir = std::env::temp_dir().join(format!("submarine-ai-test-{}", channel::random_hex(6)));
    let exec: Executor = Arc::new(|_, _, _, _| Box::pin(async { Err("no servers in this test".to_string()) }));
    let s = AiState::new(exec, Box::new(|| {}), Some(dir.clone()));
    s.turn_on().unwrap();
    s.start_tab("local-1", "Claude").unwrap();
    s.share("local-1", "sess-1", "xyz").unwrap();
    // As long as allowed, and all of it escaped in JSON.
    let long = "\u{1}".repeat(MAX_COMMAND);
    for deny in [false, true] {
        let (cancel_tx, cancel) = watch::channel(false);
        let (s2, cmd) = (s.clone(), long.clone());
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", &cmd, None, cancel).await });
        wait_for("the request", || s.snapshot().pending.len() == 1).await;
        if deny {
            s.decide(s.snapshot().pending[0].id, false).unwrap();
        } else {
            cancel_tx.send(true).unwrap();
        }
        assert!(req.await.unwrap().is_error);
    }
    let text = std::fs::read_to_string(dir.join(audit::FILE_NAME)).unwrap();
    let requests: Vec<&str> = text.lines().filter(|l| l.contains("\"event\":\"request\"")).collect();
    assert_eq!(requests.len(), 2);
    for l in requests {
        assert!(l.len() < MAX_LOGGED_COMMAND + 512, "{} bytes", l.len());
        assert!(l.contains("sha256 "));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// -- Against the test SSH server (it runs commands with this machine's sh) --

#[cfg(unix)]
mod ssh {
    use super::*;
    use crate::ssh_test_server;

    /// An AiState whose `sess-1` is a test server, with an audit log in a
    /// folder of its own, and a count of the page's change events.
    async fn state() -> (AiState, PathBuf, Arc<AtomicUsize>) {
        let (s, dir, changes, _) = state_and_server(ssh_test_server::ShellOpts::default()).await;
        (s, dir, changes)
    }

    /// The same, on a server run with `opts`, with its count of commands
    /// still running there.
    async fn state_and_server(opts: ssh_test_server::ShellOpts) -> (AiState, PathBuf, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let (handle, running) = exec::tests::server_with(opts).await;
        let exec: Executor = Arc::new(move |sid, cmd, limit, stop| {
            let h = handle.clone();
            Box::pin(async move {
                if sid != "sess-1" {
                    return Err("That server's tab isn't connected right now.".into());
                }
                exec::run(&h, &cmd, limit, OUTPUT_CAP, stop).await
            })
        });
        let changes = Arc::new(AtomicUsize::new(0));
        let c = changes.clone();
        let dir = std::env::temp_dir().join(format!("submarine-ai-test-{}", channel::random_hex(6)));
        let s = AiState::new(exec, Box::new(move || { c.fetch_add(1, Ordering::SeqCst); }), Some(dir.clone()));
        s.turn_on().unwrap();
        (s, dir, changes, running)
    }

    fn audit_lines(dir: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(dir.join(audit::FILE_NAME))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    async fn wait_pending(s: &AiState) -> PendingView {
        wait_for("a request", || !s.snapshot().pending.is_empty()).await;
        s.snapshot().pending[0].clone()
    }

    /// Through the helper, as Claude Code would: allowed, it runs; the
    /// answer has the exit code and the output, filtered; the log has it.
    #[tokio::test]
    async fn run_command_allowed() {
        let (s, dir, changes) = state().await;
        let (_, env) = s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let mut h = helper(var(&env, channel::ENV_ADDR), var(&env, channel::ENV_TOKEN));
        h.call(1, "initialize", json!({ "protocolVersion": "2025-06-18" })).await;
        h.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "run_command",
            "arguments": { "server": "xyz", "command": "echo hello; printf 'pass%s=hunter%s\\n' word 2; exit 4" }
        }}))
        .await;
        let p = wait_pending(&s).await;
        assert_eq!((p.server.as_str(), p.tab_label.as_str(), p.root), ("xyz", "Claude", false));
        assert!(changes.load(Ordering::SeqCst) > 0, "the panel heard");
        s.decide(p.id, true).unwrap();
        let r = h.recv().await;
        assert_eq!(r["id"], 2);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(r["result"]["isError"], false, "{text}");
        assert!(text.starts_with("Exit code: 4\n"), "{text}");
        assert!(text.contains("hello"), "{text}");
        assert!(text.contains(&format!("password={}", filter::HIDDEN)) && !text.contains("hunter2"), "{text}");
        assert!(text.contains("hid 1 item"), "{text}");
        // You see all of it.
        assert!(s.lock().history[0].output.contains("hunter2"));
        let done = &s.snapshot().history[0];
        assert!(!serde_json::to_string(&s.snapshot()).unwrap().contains("hunter2"), "outputs aren't in the panel's every update");
        assert_eq!((done.outcome, done.exit_code, done.hidden), ("done", Some(4), 1));
        let log = audit_lines(&dir);
        // Logged as you allowed it, and again once it ended.
        let req = log.iter().position(|l| l["event"] == "request").unwrap();
        let fin = log.iter().position(|l| l["event"] == "finished").unwrap();
        assert!(req < fin);
        let (req, fin) = (&log[req], &log[fin]);
        assert_eq!(req["decision"], "allowed");
        assert_eq!(req["by"], "you");
        assert!(req.get("exit_code").is_none());
        assert_eq!(fin["id"], req["id"]);
        assert_eq!(fin["exit_code"], 4);
        assert_eq!(fin["items_hidden"], 1);
        assert_eq!((&req["ai_tab"], &fin["ai_tab"]), (&json!("Claude"), &json!("Claude")));
        assert!(!std::fs::read_to_string(dir.join(audit::FILE_NAME)).unwrap().contains("hunter2"), "no output in the log");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Denied, nothing runs, and the AI is told so.
    #[tokio::test]
    async fn run_command_denied_runs_nothing() {
        let (s, dir, _) = state().await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let marker = dir.join("ran");
        std::fs::create_dir_all(&dir).unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let cmd = format!("touch '{}'", marker.display());
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", &cmd, None, cancel).await });
        let p = wait_pending(&s).await;
        s.decide(p.id, false).unwrap();
        let r = req.await.unwrap();
        assert!(r.is_error);
        assert_eq!(r.text, "Denied by the user.");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!marker.exists(), "nothing ran");
        let log = audit_lines(&dir);
        assert!(log.iter().any(|l| l["event"] == "request" && l["decision"] == "denied"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Past its timeout, the command is stopped and the AI told.
    #[tokio::test]
    async fn run_command_times_out() {
        let (s, dir, _) = state().await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "echo begun; sleep 30", Some(1), cancel).await });
        let p = wait_pending(&s).await;
        assert_eq!(p.timeout_secs, 1);
        s.decide(p.id, true).unwrap();
        let r = tokio::time::timeout(Duration::from_secs(10), req).await.expect("stopped in time").unwrap();
        assert!(r.text.contains("Stopped by Submarine after 1 seconds") && r.text.contains("begun"), "{}", r.text);
        assert!(audit_lines(&dir).iter().any(|l| l["note"] == "timed out"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A command with sudo in it is marked as asking for root.
    #[tokio::test]
    async fn root_words_are_marked_for_you() {
        let (s, dir, _) = state().await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "sudo -n cat /etc/shadow", None, cancel).await });
        let p = wait_pending(&s).await;
        assert!(p.root);
        s.decide(p.id, false).unwrap();
        req.await.unwrap();
        assert!(audit_lines(&dir).iter().any(|l| l["event"] == "request" && l["root"] == true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The card says who a command runs as, and a root login is root
    /// whatever the command.
    #[tokio::test]
    async fn the_login_user_is_on_the_card_and_root_is_marked() {
        let (s, dir, _) = state().await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share_as("local-1", "sess-1", "xyz", Some("root".into())).unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "uptime", None, cancel).await });
        let p = wait_pending(&s).await;
        assert_eq!((p.user.as_deref(), p.root), (Some("root"), true));
        s.decide(p.id, false).unwrap();
        req.await.unwrap();
        assert!(audit_lines(&dir).iter().any(|l| l["event"] == "request" && l["root"] == true && l["user"] == "root"));
        // Another login isn't, unless the command asks.
        s.start_tab("local-2", "Claude 2").unwrap();
        s.share_as("local-2", "sess-1", "xyz", Some("deploy".into())).unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-2", "xyz", "uptime", None, cancel).await });
        let p = wait_pending(&s).await;
        assert_eq!((p.user.as_deref(), p.root), (Some("deploy"), false));
        s.decide(p.id, false).unwrap();
        req.await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Closing the AI tab stops what it's running.
    #[tokio::test]
    async fn closing_the_ai_tab_stops_its_command() {
        let (s, dir, _) = state().await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "sleep 30", None, cancel).await });
        let p = wait_pending(&s).await;
        s.decide(p.id, true).unwrap();
        wait_for("it to run", || !s.snapshot().running.is_empty()).await;
        s.end_tab("local-1");
        let r = tokio::time::timeout(Duration::from_secs(5), req).await.expect("stopped").unwrap();
        assert!(r.is_error && r.text.starts_with("Stopped"), "{}", r.text);
        assert!(s.snapshot().running.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Stop, unsharing, closing the AI tab and turning AI tools off end the
    /// command on the server itself, not only Submarine's wait for it: also
    /// on a server (like Dropbear) that keeps a command running when its
    /// channel closes and only ends it on a signal.
    #[tokio::test]
    async fn ending_access_ends_the_command_on_the_server() {
        for (how, keep_running_on_close) in [("stop", false), ("unshare", true), ("close", false), ("off", true), ("gone", false)] {
            let opts = ssh_test_server::ShellOpts { keep_running_on_close, ..Default::default() };
            let (s, dir, _, running) = state_and_server(opts).await;
            s.start_tab("local-1", "Claude").unwrap();
            s.share("local-1", "sess-1", "xyz").unwrap();
            let (_c, cancel) = watch::channel(false);
            let s2 = s.clone();
            let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "echo begun; exec sleep 30", None, cancel).await });
            let p = wait_pending(&s).await;
            s.decide(p.id, true).unwrap();
            wait_for("it to run on the server", || running.load(Ordering::SeqCst) == 1).await;
            match how {
                "stop" => s.stop_tab("local-1"),
                "unshare" => s.unshare("sess-1", Some("local-1"), "stopped sharing"),
                "close" => s.end_tab("local-1"),
                "off" => s.turn_off().await,
                _ => s.session_gone("sess-1", "reconnected"),
            }
            let r = tokio::time::timeout(Duration::from_secs(10), req).await.expect("stopped").unwrap();
            assert!(r.is_error && r.text.starts_with("Stopped"), "{how}: {}", r.text);
            wait_for("the command to end on the server", || running.load(Ordering::SeqCst) == 0).await;
            assert!(audit_lines(&dir).iter().any(|l| l["note"] == "stopped: access ended"), "{how}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Allowed, a command is in the log while it runs, not only once it ends.
    #[tokio::test]
    async fn allowed_is_logged_before_it_ends() {
        let (s, dir, _, running) = state_and_server(Default::default()).await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (_c, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "exec sleep 30", None, cancel).await });
        let p = wait_pending(&s).await;
        s.decide(p.id, true).unwrap();
        wait_for("it to run on the server", || running.load(Ordering::SeqCst) == 1).await;
        let log = audit_lines(&dir);
        assert!(log.iter().any(|l| l["event"] == "request" && l["decision"] == "allowed" && l["id"] == p.id), "{log:?}");
        assert!(!log.iter().any(|l| l["event"] == "finished"));
        s.stop_tab("local-1");
        tokio::time::timeout(Duration::from_secs(10), req).await.expect("stopped").unwrap();
        assert!(audit_lines(&dir).iter().any(|l| l["event"] == "finished" && l["id"] == p.id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The AI withdrawing a running request ends it on the server too.
    #[tokio::test]
    async fn the_ai_withdrawing_a_running_request_ends_it() {
        let (s, dir, _, running) = state_and_server(Default::default()).await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (cancel_tx, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "exec sleep 30", None, cancel).await });
        let p = wait_pending(&s).await;
        s.decide(p.id, true).unwrap();
        wait_for("it to run on the server", || running.load(Ordering::SeqCst) == 1).await;
        cancel_tx.send(true).unwrap();
        let r = tokio::time::timeout(Duration::from_secs(10), req).await.expect("stopped").unwrap();
        assert_eq!(r.text, "Stopped: the request was withdrawn.");
        wait_for("the command to end on the server", || running.load(Ordering::SeqCst) == 0).await;
        assert!(audit_lines(&dir).iter().any(|l| l["note"] == "stopped: withdrawn by the AI tool"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Allow and an end of access at the same moment: whichever comes
    /// first, the command never runs on a server no longer shared.
    #[tokio::test]
    async fn allow_racing_an_end_of_access_never_runs() {
        for allow_first in [true, false] {
            let (s, dir, _, running) = state_and_server(Default::default()).await;
            s.start_tab("local-1", "Claude").unwrap();
            s.share("local-1", "sess-1", "xyz").unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let marker = dir.join("ran");
            let cmd = format!("touch '{}'; sleep 30", marker.display());
            let (_c, cancel) = watch::channel(false);
            let s2 = s.clone();
            let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", &cmd, None, cancel).await });
            let p = wait_pending(&s).await;
            // Both before the request's task wakes up: in the old code an
            // allowed request was then in neither list, and ran anyway.
            if allow_first {
                s.decide(p.id, true).unwrap();
                s.session_gone("sess-1", "reconnected");
            } else {
                s.unshare("sess-1", Some("local-1"), "stopped sharing");
                assert!(s.decide(p.id, true).is_err());
            }
            let r = tokio::time::timeout(Duration::from_secs(10), req).await.expect("ended").unwrap();
            assert!(r.is_error, "{}", r.text);
            assert!(r.text.starts_with(if allow_first { "Stopped" } else { "Cancelled" }), "{}", r.text);
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert!(!marker.exists(), "it never ran");
            assert_eq!(running.load(Ordering::SeqCst), 0);
            assert!(s.snapshot().running.is_empty() && s.snapshot().pending.is_empty());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Allowed just as the AI withdraws it: logged as allowed and stopped by
    /// the AI, not as "access ended", and it never runs.
    #[tokio::test]
    async fn allow_racing_a_withdrawal_is_logged_as_such() {
        let (s, dir, _, running) = state_and_server(Default::default()).await;
        s.start_tab("local-1", "Claude").unwrap();
        s.share("local-1", "sess-1", "xyz").unwrap();
        let (cancel_tx, cancel) = watch::channel(false);
        let s2 = s.clone();
        let req = tokio::spawn(async move { s2.run_command("local-1", "xyz", "sleep 30", None, cancel).await });
        let p = wait_pending(&s).await;
        s.decide(p.id, true).unwrap();
        cancel_tx.send(true).unwrap();
        let r = tokio::time::timeout(Duration::from_secs(10), req).await.expect("ended").unwrap();
        assert_eq!(r.text, "Stopped: the request was withdrawn.");
        assert_eq!(running.load(Ordering::SeqCst), 0);
        let log = audit_lines(&dir);
        let req = log.iter().find(|l| l["event"] == "request").unwrap();
        assert_eq!(req["decision"], "allowed");
        let fin = log.iter().find(|l| l["event"] == "finished").unwrap();
        assert_eq!(fin["note"], "stopped: withdrawn by the AI tool");
        assert!(!log.iter().any(|l| l["note"] == "access ended"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

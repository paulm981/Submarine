//! `submarine --mcp`, the real program: it talks MCP over its own stdin and
//! stdout to a stand-in for the app's channel (a Unix socket, or a named pipe
//! on Windows). Run on Windows as a release build too (check.yml), where
//! Submarine is a windowed program, so stdin and stdout are tested there as
//! the AI tool will use them.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const EXE: &str = env!("CARGO_BIN_EXE_submarine");

/// The stand-in: takes one connection, answers its hello (`ok` when the
/// token is "good"), then answers each line with `{"echo": <line>}`.
async fn stand_in<S>(stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut s = tokio::io::BufReader::new(stream);
    let mut hello = String::new();
    s.read_line(&mut hello).await.unwrap();
    let hello: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(hello["submarine_ai"], 1);
    if hello["token"] != "good" {
        s.get_mut().write_all(b"{\"ok\":false,\"error\":\"not this one\"}\n").await.unwrap();
        return;
    }
    s.get_mut().write_all(b"{\"ok\":true}\n").await.unwrap();
    loop {
        let mut line = String::new();
        if s.read_line(&mut line).await.unwrap_or(0) == 0 {
            break;
        }
        let reply = serde_json::json!({ "echo": line.trim_end() }).to_string() + "\n";
        s.get_mut().write_all(reply.as_bytes()).await.unwrap();
    }
}

/// Listen at a new address; the stand-in serves the first connection.
#[cfg(unix)]
fn listen() -> (String, tokio::task::JoinHandle<()>) {
    let dir = std::env::temp_dir().join(format!("submarine-helper-test-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s");
    let l = tokio::net::UnixListener::bind(&path).unwrap();
    let task = tokio::spawn(async move {
        let (s, _) = l.accept().await.unwrap();
        stand_in(s).await;
    });
    (path.to_string_lossy().into_owned(), task)
}

#[cfg(windows)]
fn listen() -> (String, tokio::task::JoinHandle<()>) {
    let name = format!(r"\\.\pipe\submarine-ai-test-{}-{}", std::process::id(), rand_suffix());
    let server = tokio::net::windows::named_pipe::ServerOptions::new().first_pipe_instance(true).create(&name).unwrap();
    let task = tokio::spawn(async move {
        server.connect().await.unwrap();
        stand_in(server).await;
    });
    (name, task)
}

/// A name part no other test in this run has: the clock alone isn't enough,
/// since tests run at once and macOS's clock ticks in microseconds.
fn rand_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{}-{}", nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

fn helper(addr: Option<&str>, token: Option<&str>) -> std::process::Child {
    let mut c = Command::new(EXE);
    c.arg("--mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.env_remove("SUBMARINE_AI_ADDR").env_remove("SUBMARINE_AI_TOKEN");
    if let Some(a) = addr {
        c.env("SUBMARINE_AI_ADDR", a);
    }
    if let Some(t) = token {
        c.env("SUBMARINE_AI_TOKEN", t);
    }
    c.spawn().unwrap()
}

/// Wait for the helper to end, at most 20 seconds.
fn finish(mut child: std::process::Child) -> (i32, String, String) {
    let t = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut out = String::new();
            let mut err = String::new();
            std::io::Read::read_to_string(child.stdout.as_mut().unwrap(), &mut out).unwrap();
            std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
            return (status.code().unwrap_or(-1), out, err);
        }
        if t.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("the helper didn't end");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn it_passes_mcp_lines_through_stdin_and_stdout() {
    let (addr, server) = listen();
    let mut child = helper(Some(&addr), Some("good"));
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let lines = tokio::task::spawn_blocking(move || {
        let mut got = Vec::new();
        for msg in [r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#] {
            writeln!(stdin, "{msg}").unwrap();
            stdin.flush().unwrap();
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            got.push(line);
        }
        // Closing stdin ends it.
        drop(stdin);
        (got, stdout)
    })
    .await
    .unwrap();
    let (got, stdout) = lines;
    let first: serde_json::Value = serde_json::from_str(&got[0]).unwrap();
    assert_eq!(first["echo"], r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    let second: serde_json::Value = serde_json::from_str(&got[1]).unwrap();
    assert_eq!(second["echo"], r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    child.stdout = Some(stdout.into_inner());
    let (code, rest, err) = tokio::task::spawn_blocking(move || finish(child)).await.unwrap();
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(rest, "", "nothing else on stdout");
    server.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_ends_it_with_the_reason() {
    let (addr, server) = listen();
    let child = helper(Some(&addr), Some("bad"));
    let (code, out, err) = tokio::task::spawn_blocking(move || finish(child)).await.unwrap();
    assert_eq!(code, 1);
    assert_eq!(out, "", "nothing on stdout");
    assert!(err.contains("not this one"), "{err}");
    server.await.unwrap();
}

#[test]
fn started_outside_submarine_it_connects_to_nothing() {
    let child = helper(None, None);
    let (code, out, err) = finish(child);
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert!(err.contains("only an AI tool started from Submarine"), "{err}");
}

#[test]
fn with_nothing_listening_it_says_so() {
    #[cfg(unix)]
    let addr = std::env::temp_dir().join("submarine-ai-none/s").to_string_lossy().into_owned();
    #[cfg(windows)]
    let addr = r"\\.\pipe\submarine-ai-none-listening".to_string();
    let (code, out, err) = finish(helper(Some(&addr), Some("x")));
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert!(err.contains("Couldn't reach Submarine"), "{err}");
}

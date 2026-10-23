//! The app's own log file, so a freeze, a dropped connection or an error can
//! be looked into after the fact.
//!
//! What goes in, each line with the time:
//! - everything the app prints: the connection steps, sync and transfer
//!   notes. A windowed app on Windows has no console, so without this they
//!   went nowhere. Standard output and error are routed through a pipe
//!   (`capture_std_streams`) and still echoed to the console when there is one.
//! - a crash: the message and a backtrace (`install_panic_hook`).
//! - what the page reports: its errors and the freeze watchdog's notes
//!   (`app_log`, and src/util/diagnostics.ts).
//! - the backend's side of the watchdog: when the window stops answering
//!   (`ui_heartbeat`), how long for, and which terminals were receiving the
//!   most output then (`count_output`).
//!
//! The file is `submarine.log` in the app's log folder (inside
//! `submarine-data` in portable mode). Past 2 MB it becomes
//! `submarine.old.log` and a new one starts, so the two stay under 4 MB.
//! Settings → Troubleshooting opens the folder, and "Copy diagnostics"
//! (`diagnostics_report`) copies the build, the system and the end of the log
//! with server names, host names, addresses and user names replaced, ready to
//! paste to whoever is helping.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const FILE_NAME: &str = "submarine.log";
const OLD_NAME: &str = "submarine.old.log";
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// A longer line (a runaway print) is cut, so one can't fill the file.
const MAX_LINE: usize = 8 * 1024;

struct Sink {
    dir: PathBuf,
    file: Option<File>,
    size: u64,
}

impl Sink {
    fn open(dir: PathBuf) -> Sink {
        let path = dir.join(FILE_NAME);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut sink = Sink { dir, file: None, size };
        if size >= MAX_BYTES {
            sink.rotate();
        } else {
            sink.file = OpenOptions::new().create(true).append(true).open(&path).ok();
        }
        sink
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = std::fs::rename(self.dir.join(FILE_NAME), self.dir.join(OLD_NAME));
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.dir.join(FILE_NAME))
            .ok();
        self.size = 0;
    }

    fn write(&mut self, line: &str) {
        if self.size >= MAX_BYTES {
            self.rotate();
        }
        if let Some(f) = self.file.as_mut() {
            if f.write_all(line.as_bytes()).is_ok() {
                self.size += line.len() as u64;
            }
        }
    }
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();
/// Set once stdout and stderr go to the log: the crash hook then writes the
/// crash itself instead of leaving it to the default hook's stderr print.
static CAPTURED: OnceLock<()> = OnceLock::new();

/// Start the log in `dir`. Called once, from the app's setup; later calls do
/// nothing. Lines printed before it are only on the console.
pub fn start(dir: PathBuf) {
    if SINK.get().is_some() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    if SINK.set(Mutex::new(Sink::open(dir))).is_err() {
        return;
    }
    write_line(
        "app",
        &format!(
            "Submarine {} started ({} {})",
            crate::about::build_label(),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    install_panic_hook();
    if capture_std_streams() {
        let _ = CAPTURED.set(());
    }
    start_watchdog();
}

/// The folder the log is in, once it's started.
pub fn dir() -> Option<PathBuf> {
    SINK.get().and_then(|s| s.lock().ok().map(|s| s.dir.clone()))
}

fn format_line(time: &str, source: &str, text: &str) -> String {
    let text = text.trim_end_matches(['\n', '\r']);
    let text = if text.len() > MAX_LINE {
        let mut end = MAX_LINE;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}… ({} bytes cut)", &text[..end], text.len() - end)
    } else {
        text.to_string()
    };
    format!("{} [{}] {}\n", time, source, text)
}

fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// What tests wrote to the log, which tests never start: to check that
/// something (a password) never reaches it.
#[cfg(test)]
pub(crate) static TEST_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// One line in the log, from `source` (app, ui, watchdog, crash).
pub fn write_line(source: &str, text: &str) {
    #[cfg(test)]
    if let Ok(mut lines) = TEST_LINES.lock() {
        if lines.len() < 100_000 {
            lines.push(format!("[{source}] {text}"));
        }
    }
    let Some(sink) = SINK.get() else { return };
    let line = format_line(&now(), source, text);
    if let Ok(mut s) = sink.lock() {
        s.write(&line);
    }
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if CAPTURED.get().is_none() {
            default(info);
        }
        let backtrace = std::backtrace::Backtrace::force_capture();
        // try_lock: a crash inside a log write must not wait on itself.
        if let Some(Ok(mut s)) = SINK.get().map(|s| s.try_lock()) {
            s.write(&format_line(&now(), "crash", &format!("{info}\n{backtrace}")));
        }
    }));
}

/// Route standard output and error into the log through a pipe read on its
/// own thread, echoing to the console the app had (if any).
#[cfg(unix)]
fn capture_std_streams() -> bool {
    use std::os::fd::FromRawFd;
    // SAFETY: plain fd plumbing on fds this function creates; fds 1 and 2
    // are replaced atomically by dup2, and the pipe's read end is owned by
    // the File handed to the reader thread.
    unsafe {
        let mut fds = [0 as libc::c_int; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return false;
        }
        let console = libc::dup(2);
        if libc::dup2(fds[1], 1) < 0 || libc::dup2(fds[1], 2) < 0 {
            if console >= 0 {
                libc::dup2(console, 2);
                libc::close(console);
            }
            libc::close(fds[0]);
            libc::close(fds[1]);
            return false;
        }
        libc::close(fds[1]);
        // Programs the app starts keep their own copy of fds 1 and 2 (what
        // they print lands in the log too), but not of the read end.
        libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC);
        let reader = File::from_raw_fd(fds[0]);
        let echo = (console >= 0).then(|| File::from_raw_fd(console));
        spawn_reader(reader, echo);
    }
    true
}

#[cfg(windows)]
fn capture_std_streams() -> bool {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    use windows_sys::Win32::System::Pipes::CreatePipe;
    // SAFETY: CreatePipe fills both handles on success; each is then owned by
    // exactly one thing (the std handles keep the write end for the life of
    // the process, the reader thread's File owns the read end).
    unsafe {
        let mut read: HANDLE = std::ptr::null_mut();
        let mut write: HANDLE = std::ptr::null_mut();
        if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
            return false;
        }
        // A windowed app has no console: null. A debug build run from a
        // terminal has one, and keeps seeing the output.
        let console = GetStdHandle(STD_ERROR_HANDLE);
        if SetStdHandle(STD_OUTPUT_HANDLE, write) == 0 || SetStdHandle(STD_ERROR_HANDLE, write) == 0 {
            return false;
        }
        let reader = File::from_raw_handle(read as _);
        let echo = (!console.is_null() && console != INVALID_HANDLE_VALUE).then(|| File::from_raw_handle(console as _));
        spawn_reader(reader, echo);
    }
    true
}

#[cfg(not(any(unix, windows)))]
fn capture_std_streams() -> bool {
    false
}

#[cfg(any(unix, windows))]
fn spawn_reader(reader: File, mut echo: Option<File>) {
    let _ = std::thread::Builder::new().name("log-capture".into()).spawn(move || {
        // Never stops reading while the app runs: a full pipe would block
        // every print in the app.
        let mut lines = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match lines.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(e) = echo.as_mut() {
                        if e.write_all(&buf).is_err() {
                            echo = None;
                        }
                    }
                    write_line("app", &String::from_utf8_lossy(&buf));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Watchdog: the window stopped answering
// ---------------------------------------------------------------------------
//
// The page calls `ui_heartbeat` every second while it's shown. If none comes
// for WATCHDOG_AFTER while it should, the page's thread is stuck (the whole
// window is one thread), and the log says so with the terminals that were
// receiving the most output, which the page can't report while it's stuck.

const WATCHDOG_AFTER: Duration = Duration::from_secs(5);
/// How far back "the most output" looks.
const RATE_WINDOW: Duration = Duration::from_secs(10);

struct Beat {
    last: Option<Instant>,
    /// The window is hidden or minimized, where timers are slowed down: no
    /// heartbeat then doesn't mean stuck.
    hidden: bool,
    /// When the current silence was logged, so it's logged once.
    stuck_since: Option<Instant>,
}

static BEAT: Mutex<Beat> = Mutex::new(Beat { last: None, hidden: false, stuck_since: None });

/// Output per terminal: (bytes in the current window, when it began).
static OUTPUT: OnceLock<Mutex<HashMap<String, (u64, Instant)>>> = OnceLock::new();

/// A batch of `bytes` sent to terminal `terminal_id` (term_output.rs).
pub fn count_output(terminal_id: &str, bytes: usize) {
    let map = OUTPUT.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut m) = map.lock() {
        let now = Instant::now();
        let entry = m.entry(terminal_id.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) > RATE_WINDOW {
            *entry = (0, now);
        }
        entry.0 += bytes as u64;
    }
}

/// The terminals that received the most output in the last RATE_WINDOW, for
/// a log line: "session-3-term-0 41.2 MB, home-1 0.3 MB".
pub fn busiest_terminals() -> String {
    let Some(map) = OUTPUT.get() else { return "none".into() };
    let Ok(m) = map.lock() else { return "unknown".into() };
    let now = Instant::now();
    let mut rows: Vec<(&String, u64)> = m
        .iter()
        .filter(|(_, (bytes, since))| *bytes > 0 && now.duration_since(*since) <= RATE_WINDOW * 2)
        .map(|(id, (bytes, _))| (id, *bytes))
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));
    if rows.is_empty() {
        return "none".into();
    }
    rows.iter()
        .take(3)
        .map(|(id, bytes)| format!("{} {}", id, human_bytes(*bytes)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn human_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{} B", n)
    }
}

fn start_watchdog() {
    let _ = std::thread::Builder::new().name("ui-watchdog".into()).spawn(|| {
        let mut tick = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            // A one-second sleep that took much longer: the computer was
            // asleep, and so was the window. Start counting again.
            let slept = tick.elapsed();
            tick = Instant::now();
            let Ok(mut b) = BEAT.lock() else { return };
            if slept > Duration::from_secs(3) {
                if b.last.is_some() {
                    b.last = Some(tick);
                }
                continue;
            }
            let (Some(last), false) = (b.last, b.hidden) else { continue };
            let silent = last.elapsed();
            if silent >= WATCHDOG_AFTER && b.stuck_since.is_none() {
                b.stuck_since = Some(last);
                drop(b);
                write_line(
                    "watchdog",
                    &format!(
                        "the window hasn't answered for {} s; most terminal output in the last {} s: {}",
                        silent.as_secs(),
                        RATE_WINDOW.as_secs(),
                        busiest_terminals()
                    ),
                );
            }
        }
    });
}

/// The page's heartbeat; `visible` is false while the window is hidden or
/// minimized.
#[tauri::command]
pub fn ui_heartbeat(visible: bool) {
    let now = Instant::now();
    let recovered = {
        let Ok(mut b) = BEAT.lock() else { return };
        b.last = Some(now);
        b.hidden = !visible;
        b.stuck_since.take()
    };
    if let Some(since) = recovered {
        write_line(
            "watchdog",
            &format!("the window answers again, after {:.1} s", now.duration_since(since).as_secs_f32()),
        );
    }
}

/// A line from the page: its errors and its own freeze notes.
#[tauri::command]
pub fn app_log(level: String, message: String) {
    let level = match level.as_str() {
        "error" | "warn" | "info" => level,
        _ => "info".to_string(),
    };
    write_line(&format!("ui {}", level), &message);
}

/// Open the log folder in the system's file manager.
#[tauri::command]
pub fn open_log_folder() -> Result<(), String> {
    let dir = dir().ok_or("The log hasn't started")?;
    #[cfg(target_os = "macos")]
    {
        // The folder is named after the app's id (`com.submarine.app`), and a
        // folder ending in .app is an application to macOS: `open` tries to
        // launch it and fails. Finder shows the log file in it instead.
        let file = dir.join(FILE_NAME);
        let target = if file.exists() { file } else { dir.clone() };
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&target)
            .status()
            .map_err(|e| format!("Couldn't open {}: {e}", dir.display()))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Couldn't show {} in Finder.", target.display()))
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "macos")))]
    {
        open::that_detached(&dir).map_err(|e| format!("Couldn't open {}: {e}", dir.display()))
    }
    #[cfg(target_os = "android")]
    {
        Err(format!("The log is in {}", dir.display()))
    }
}

/// The last `max_lines` lines of the log: the older file's end first when
/// the current one is short.
pub fn tail(max_lines: usize) -> String {
    let Some(dir) = dir() else { return String::new() };
    let read = |p: &Path| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let current = read(&dir.join(FILE_NAME));
    let mut lines: Vec<&str> = current.lines().collect();
    let old;
    if lines.len() < max_lines {
        old = read(&dir.join(OLD_NAME));
        let mut older: Vec<&str> = old.lines().collect();
        let want = max_lines - lines.len();
        let skip = older.len().saturating_sub(want);
        older.drain(..skip);
        older.extend(lines);
        lines = older;
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines[skip..].join("\n")
}

// ---------------------------------------------------------------------------
// Redaction for "Copy diagnostics"
// ---------------------------------------------------------------------------

/// `text` with every name in `names` (a value and what it is: "server",
/// "host", "user", "folder"...) replaced by a numbered placeholder, the same
/// value always by the same one, longest first so a name inside another is
/// handled whole. Then IPv4 addresses (loopback kept) and e-mail addresses,
/// and `home` (the user's home folder) as `~`.
pub fn redact(text: &str, names: &[(String, &'static str)], home: Option<&str>) -> String {
    let mut out = text.to_string();
    if let Some(home) = home.filter(|h| h.len() > 3) {
        out = out.replace(home, "~");
    }
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut sorted: Vec<&(String, &'static str)> = names.iter().filter(|(v, _)| v.trim().len() >= 3).collect();
    sorted.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
    for (value, kind) in sorted {
        if seen.iter().any(|(v, _)| v == value) || !out.contains(value.as_str()) {
            continue;
        }
        let n = counts.entry(kind).or_insert(0);
        *n += 1;
        let placeholder = format!("<{}-{}>", kind, n);
        out = out.replace(value.as_str(), &placeholder);
        seen.push((value.clone(), placeholder));
    }
    redact_emails(&redact_ipv4(&out))
}

fn redact_ipv4(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let starts_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.');
        if starts_word && bytes[i].is_ascii_digit() {
            let mut j = i;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                j += 1;
            }
            let candidate = &text[i..j];
            let candidate = candidate.trim_end_matches('.');
            let parts: Vec<&str> = candidate.split('.').collect();
            let is_ip = parts.len() == 4
                && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255));
            let ends_word = j >= bytes.len() || !bytes[j].is_ascii_alphanumeric();
            if is_ip && ends_word {
                if candidate.starts_with("127.") || candidate == "0.0.0.0" {
                    out.push_str(candidate);
                } else {
                    out.push_str("<ip>");
                }
                i += candidate.len();
                continue;
            }
            out.push_str(&text[i..j]);
            i = j;
            continue;
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_emails(text: &str) -> String {
    let is_part = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let before = &rest[..at];
        let start = before.rfind(|c: char| !is_part(c)).map(|p| p + 1).unwrap_or(0);
        let after = &rest[at + 1..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(after.len());
        let domain = after[..end].trim_end_matches('.');
        if start < at && domain.contains('.') {
            out.push_str(&before[..start]);
            out.push_str("<email>");
            rest = &after[domain.len()..];
        } else {
            out.push_str(&rest[..=at]);
            rest = &rest[at + 1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_has_the_time_and_source_and_a_runaway_one_is_cut() {
        assert_eq!(format_line("T", "app", "hello\n"), "T [app] hello\n");
        let long = "é".repeat(MAX_LINE);
        let line = format_line("T", "app", &long);
        assert!(line.len() < MAX_LINE + 100);
        assert!(line.contains("bytes cut"));
    }

    #[test]
    fn the_log_rotates_past_its_size() {
        let dir = std::env::temp_dir().join(format!("submarine-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut sink = Sink::open(dir.clone());
        let big = "x".repeat(1024 * 1024) + "\n";
        sink.write(&big);
        sink.write(&big);
        sink.write("after\n");
        assert!(dir.join(OLD_NAME).exists(), "the full file was kept as the old one");
        assert_eq!(std::fs::read_to_string(dir.join(FILE_NAME)).unwrap(), "after\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_addresses_and_emails_are_replaced() {
        let names = vec![
            ("prod-db".to_string(), "server"),
            ("prod-db-replica".to_string(), "server"),
            ("db1.corp.example".to_string(), "host"),
            ("alice".to_string(), "user"),
            ("ab".to_string(), "user"),
        ];
        let text = "[LOG] connect alice@db1.corp.example (prod-db-replica, then prod-db) via 10.1.2.3:22 \
                    and 127.0.0.1; v1.2.3.4.5 stays; mail bob@mail.example; /home/me/x; ab";
        let out = redact(text, &names, Some("/home/me"));
        assert!(!out.contains("prod-db"), "{out}");
        assert!(out.contains("<server-1>") && out.contains("<server-2>"), "{out}");
        assert!(!out.contains("db1.corp"), "{out}");
        assert!(!out.contains("alice"), "{out}");
        assert!(!out.contains("10.1.2.3"), "{out}");
        assert!(out.contains("127.0.0.1"), "{out}");
        assert!(out.contains("v1.2.3.4.5"), "{out}");
        assert!(out.contains("<email>"), "{out}");
        assert!(out.contains("~/x"), "{out}");
        assert!(out.ends_with("ab"), "two letters are too short to replace safely: {out}");
        // The same value always gets the same placeholder.
        let twice = redact("prod-db and prod-db", &names, None);
        assert_eq!(twice, "<server-1> and <server-1>");
    }

    #[test]
    fn output_is_counted_per_terminal() {
        count_output("t-test-a", 3 * 1024 * 1024);
        count_output("t-test-b", 10);
        let busiest = busiest_terminals();
        assert!(busiest.starts_with("t-test-a 3.0 MB"), "{busiest}");
    }
}

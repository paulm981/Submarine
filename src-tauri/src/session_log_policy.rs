//! The fork's bounds on the original project's session logs (session_log.rs,
//! their #403; #288), kept out of their code, which calls in here from
//! one-line hooks (`the_hooks_are_in_place` below).
//!
//! A log holds whatever the terminal showed: what a server printed, the
//! command lines the shell echoed (a secret Library command's text among
//! them), a password if a prompt echoed it. So:
//!
//! - **Only you can read it** (Linux and macOS): a log is made 0600 and the
//!   folders made for it 0700, when they're made (`owner_only_dir`, `open`),
//!   not changed afterwards. A folder or log that's already there keeps its
//!   own permissions. On Windows a file in your Documents already takes your
//!   own access list.
//! - **At most 1 GB a log** (`LIMIT`): a `yes`, or a flood with "All output",
//!   would otherwise fill the disk. Past it the log writes one line saying it
//!   stopped, and stops; the terminal goes on (`Capped`). Refused, not
//!   rotated.
//! - **A name can't leave the log folder** (`check_name`): their `file_name`
//!   already takes separators, `..` and control characters out of what a code
//!   puts in. This checks what comes out: plain names only, under the folder,
//!   no NUL, and on Windows no device name (`CON`, `NUL`, `COM1`...), which
//!   would write nowhere.
//!
//! The pump's side doesn't change: with no log it's one check a packet, and
//! with one a copy and a send that never waits (session_log.rs). The budget
//! test below holds the pump to that while the log's file is stuck.

use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};

/// The most a log file takes: 1 GiB.
static LIMIT: u64 = 1 << 30;

/// The line a log ends with when it reaches `LIMIT`.
static STOPPED_NOTE: &str = "\n=== Submarine: this log reached 1 GB and stopped here; the session went on ===\n";

/// Why the log stopped, for the terminal's line (session_log's `Failed`).
static STOPPED: &str = "it reached 1 GB, the most a log takes, so it stopped. The terminal goes on.";

/// The folders a log goes in are made only you can open (0700), on Linux and
/// macOS: a log holds what the screen showed.
pub(crate) fn owner_only_dir(builder: &mut DirBuilder) -> &mut DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

/// Opens a log as `options` say, made only you can read (0600) on Linux and
/// macOS, and held to `LIMIT` from what the file already has.
pub(crate) fn open(options: &mut OpenOptions, path: &Path) -> io::Result<Capped<File>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let written = file.metadata()?.len();
    Ok(Capped { inner: file, written, limit: limit_for(path), stopped: false })
}

fn limit_for(_path: &Path) -> u64 {
    #[cfg(test)]
    if let Some(limit) = tests::limit_for(_path) {
        return limit;
    }
    LIMIT
}

/// A log's file, held to a size: a write that would take it past the limit
/// isn't made; the file gets `STOPPED_NOTE` and the write an error, so the
/// log thread says so and ends (session_log's `run`).
pub(crate) struct Capped<W> {
    inner: W,
    written: u64,
    limit: u64,
    stopped: bool,
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.stopped {
            return Err(io::Error::other(STOPPED));
        }
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(len) > self.limit {
            self.stopped = true;
            // Best effort: the log stops whether or not the note fits.
            let _ = self.inner.write_all(STOPPED_NOTE.as_bytes());
            return Err(io::Error::other(STOPPED));
        }
        let n = self.inner.write(buf)?;
        self.written = self.written.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Refuses a log's path that isn't a plain name under `folder`, has a NUL,
/// or, on Windows, is a device name.
pub(crate) fn check_name(folder: &Path, path: &Path) -> Result<(), String> {
    check_name_for(folder, path, cfg!(windows))
}

fn check_name_for(folder: &Path, path: &Path, windows: bool) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("The session log's file name {why}: {}. Change the file name in Settings.", path.display()));
    let Ok(rel) = path.strip_prefix(folder) else { return refuse("isn't in the log folder") };
    if rel.as_os_str().is_empty() {
        return refuse("is empty");
    }
    for part in rel.components() {
        let Component::Normal(name) = part else { return refuse("leaves the log folder") };
        let name = name.to_string_lossy();
        if name.contains('\0') {
            return refuse("has a NUL character");
        }
        if windows && is_device_name(&name) {
            return refuse("is one Windows keeps for a device");
        }
    }
    Ok(())
}

/// Whether Windows takes `name` for a device, whatever its extension:
/// `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9` (and their
/// superscript ¹²³), `CONIN$`, `CONOUT$`, in any case, with spaces before the
/// dot ("NUL .log").
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let Some(rest) = stem.strip_prefix("COM").or_else(|| stem.strip_prefix("LPT")) else { return false };
    let mut chars = rest.chars();
    matches!((chars.next(), chars.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_log::{Event, Mode, SessionLog, Target};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// Smaller limits for the logs some tests write, by path.
    static LIMITS: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

    pub(super) fn limit_for(path: &Path) -> Option<u64> {
        LIMITS.lock().ok()?.iter().find(|(p, _)| p == path).map(|(_, l)| *l)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sub-log-policy-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A log on `target`, with its events on a channel.
    fn start(target: Target) -> (SessionLog, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let log = SessionLog::start(target, move |e| {
            let _ = tx.send(e);
        });
        (log, rx)
    }

    /// Waits for the log thread to end: its channel closes with it.
    fn events_until_done(rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(20)) {
            out.push(e);
        }
        out
    }

    #[test]
    fn the_hooks_are_in_place() {
        use crate::fork_hooks::{assert_hooks, assert_in_order};
        assert_hooks("lib.rs", include_str!("lib.rs"), &[("", "mod session_log_policy;", 1)]);
        let log = include_str!("session_log.rs");
        assert_hooks(
            "session_log.rs",
            log,
            &[
                ("open", "crate::session_log_policy::owner_only_dir(fs::DirBuilder::new().recursive(true)).create(parent)?", 1),
                ("open", "fs::create_dir_all", 0),
                ("open", "-> std::io::Result<crate::session_log_policy::Capped<File>>", 1),
                ("open", "crate::session_log_policy::open(&mut options, &target.path)", 1),
                ("target", "crate::session_log_policy::check_name(&folder, &path)?", 1),
            ],
        );
        // The name is checked once the path is made, before the target is.
        assert_in_order("session_log.rs", log, "target", &["let path = folder.join(", "session_log_policy::check_name(", "Ok(Some(Target"]);
        // The fork's early output (a root container's, a kept session's
        // note) is logged with the rest, before the pump's first packet.
        let ssh = include_str!("ssh_manager.rs");
        assert_hooks("ssh_manager.rs", ssh, &[("pty_pump", "log.write(&early)", 1)]);
        assert_in_order("ssh_manager.rs", ssh, "pty_pump", &["if !early.is_empty() {", "log.write(&early)", "limiter.filter(&early, &mut out_buf)", "read_half.wait()"]);
    }

    /// With no log, the pump's cost for each packet is one check: the data
    /// arm asks `log.as_mut()` once and does nothing else for the log, in the
    /// SSH pump and the telnet one, and nothing waits on the log anywhere.
    #[test]
    fn with_no_log_the_pump_does_one_check_a_packet() {
        use crate::fork_hooks::assert_hooks;
        let ssh = include_str!("ssh_manager.rs");
        assert_hooks(
            "ssh_manager.rs",
            ssh,
            &[
                // The early output's, once, and each packet's.
                ("pty_pump", "if let Some(log) = log.as_mut()", 2),
                ("pty_pump", "log.write(data)", 1),
                ("pty_pump", "log.write(", 2),
            ],
        );
        let telnet = include_str!("telnet.rs");
        assert_hooks(
            "telnet.rs",
            telnet,
            &[
                // The server's text, and the local echo.
                ("run_pump", "if let Some(log) = log.as_mut()", 2),
                ("run_pump", "log.write(", 2),
            ],
        );
        // And `write` itself never waits: a try_send, never a send.
        let log = include_str!("session_log.rs");
        assert_hooks("session_log.rs", log, &[("write", "self.tx.try_send(", 1), ("write", ".send(", 0)]);
    }

    /// The pump's side of a log, on a burst of output while the log's file
    /// can't be written at all (a FIFO no one reads: the log thread is stuck
    /// opening it): the pump's filter, send and log write on 40 MB in 32 KiB
    /// packets keeps the same budget as term_output's
    /// `filtering_and_sending_output_stays_fast` without a log. The queue
    /// takes what it holds and the rest is dropped and counted; nothing
    /// waits. Then the stuck file is read, and the log says what it dropped.
    #[cfg(unix)]
    #[test]
    fn a_burst_with_a_stuck_log_file_never_holds_up_the_pump() {
        let dir = temp_dir("stuck");
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("stuck.log");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(made.success(), "mkfifo");
        let target = Target { mode: Mode::All, path: fifo.clone(), append: false, header: String::new() };
        let (mut log, events) = start(target);

        let data = output(40 << 20);
        let channel = tauri::ipc::Channel::new(|_| Ok(()));
        let mut limiter = crate::term_filter::CsiLimiter::default();
        limiter.set_size(120, 40);
        let mut out = Vec::new();
        let mut slowest = Duration::ZERO;
        let t = Instant::now();
        for read in data.chunks(32 * 1024) {
            let one = Instant::now();
            log.write(read);
            slowest = slowest.max(one.elapsed());
            limiter.filter(read, &mut out);
            if out.len() >= 256 * 1024 {
                crate::term_output::send("t-log-speed", &channel, &mut out);
            }
        }
        crate::term_output::send("t-log-speed", &channel, &mut out);
        let elapsed = t.elapsed().as_secs_f64();
        let mb_s = data.len() as f64 / (1024.0 * 1024.0) / elapsed;
        println!("with a stuck log: {mb_s:.0} MB/s, slowest log write {slowest:?}");
        assert!(mb_s >= 20.0, "the pump with a stuck log: {mb_s:.0} MB/s, under term_output's budget of 20");
        assert!(slowest < Duration::from_millis(100), "a log write waited {slowest:?}");

        // Unstick it: what the queue held, then the note for what it dropped.
        let reader = std::thread::spawn(move || std::fs::read(&fifo).unwrap());
        drop(log);
        let got = reader.join().unwrap();
        let events = events_until_done(&events);
        assert!(matches!(events.first(), Some(Event::Opened(_))), "{events:?}");
        assert!(got.len() < data.len(), "some was dropped, not waited for");
        let tail = String::from_utf8_lossy(&got[got.len().saturating_sub(200)..]).into_owned();
        assert!(tail.ends_with("of output not logged: the log file was too slow ===\n"), "{tail}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Made-up log lines with a colored word, the same every time.
    fn output(bytes: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes + 128);
        let mut i = 0u32;
        while out.len() < bytes {
            out.extend_from_slice(format!("2026-11-26 10:41:{:02} \x1b[1;32mINFO\x1b[0m api request {i} handled in {} ms\r\n", i % 60, i % 900).as_bytes());
            i += 1;
        }
        out
    }

    #[cfg(unix)]
    #[test]
    fn a_log_and_its_new_folders_are_only_yours() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        let path = dir.join("a").join("b").join("s.log");
        let (mut log, events) = start(Target { mode: Mode::Text, path: path.clone(), append: false, header: String::new() });
        log.write(b"secret\n");
        drop(log);
        assert_eq!(events_until_done(&events), vec![Event::Opened(path.clone())]);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "the log");
        assert_eq!(mode(&dir.join("a").join("b")), 0o700, "its folder");
        assert_eq!(mode(&dir.join("a")), 0o700, "the folder above, made for it too");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_stops_at_its_limit_with_one_line_and_says_so() {
        let dir = temp_dir("limit");
        let path = dir.join("s.log");
        LIMITS.lock().unwrap().push((path.clone(), 4096));
        let (mut log, events) = start(Target { mode: Mode::All, path: path.clone(), append: false, header: "=== h ===\n".into() });
        let piece = [b'x'; 1000];
        for _ in 0..3 {
            log.write(&piece);
        }
        // Let the thread write those, so the next is the one past the limit.
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..20 {
            log.write(&piece);
        }
        // The log has stopped; the pump's writes go nowhere and never fail.
        std::thread::sleep(Duration::from_millis(300));
        log.write(b"after");
        drop(log);
        let events = events_until_done(&events);
        assert_eq!(events.len(), 2, "{events:?}");
        assert_eq!(events[0], Event::Opened(path.clone()));
        assert!(matches!(&events[1], Event::Failed(m) if m.contains("reached 1 GB") && m.contains("The terminal goes on")), "{events:?}");
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.len() <= 4096 + STOPPED_NOTE.len(), "{}", written.len());
        assert!(written.ends_with(STOPPED_NOTE), "{written}");
        assert_eq!(written.matches("stopped here").count(), 1);
        assert!(!written.contains("after"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capped_counts_what_the_file_had_and_refuses_past_its_limit() {
        let mut c = Capped { inner: Vec::new(), written: 90, limit: 100, stopped: false };
        c.write_all(b"0123456789").unwrap();
        assert!(c.write_all(b"x").is_err());
        assert!(c.write_all(b"y").is_err(), "stays stopped");
        assert_eq!(c.inner, [b"0123456789".as_slice(), STOPPED_NOTE.as_bytes()].concat());
        assert_eq!(LIMIT, 1024 * 1024 * 1024);
    }

    #[test]
    fn names_stay_in_the_log_folder() {
        let folder = Path::new("/home/u/logs");
        let ok = |p: &str, win| check_name_for(folder, &folder.join(p), win);
        assert_eq!(ok("web 2026-03-04 050607.log", true), Ok(()));
        assert_eq!(ok("logs/web/2026.log", true), Ok(()));
        assert!(ok("../x.log", false).is_err());
        assert!(ok("a/../../x.log", false).is_err());
        assert!(ok("./x.log", false).is_ok(), "`Path` drops a `.` in the middle");
        assert!(check_name_for(folder, Path::new("/etc/x.log"), false).is_err());
        assert!(check_name_for(folder, folder, false).is_err());
        assert!(ok("a\0b.log", false).is_err());
        // What their `file_name` makes from a hostile server name passes as
        // one plain name.
        let now = chrono::Local::now();
        let made = crate::session_log::file_name("&S.log", "../../x/\0y\\..", "h", 22, now);
        assert_eq!(check_name_for(folder, &folder.join(&made), true), Ok(()));
        assert_eq!(made.components().count(), 1);
    }

    #[test]
    fn windows_device_names_are_refused_there() {
        for name in ["CON", "nul", "NUL.log", "Aux.txt", "COM1", "lpt9.log", "COM0", "COM¹", "NUL .log", "CONOUT$"] {
            assert!(is_device_name(name), "{name}");
        }
        for name in ["CONSOLE", "nul_", "COM10", "COM", "LPTX.log", "web 1 2026.log", "_NUL.log"] {
            assert!(!is_device_name(name), "{name}");
        }
        let folder = Path::new("/home/u/logs");
        assert!(check_name_for(folder, &folder.join("NUL.log"), true).is_err());
        assert!(check_name_for(folder, &folder.join("con/web.log"), true).is_err());
        assert_eq!(check_name_for(folder, &folder.join("NUL.log"), false), Ok(()), "only on Windows");
    }
}

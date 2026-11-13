//! Following a log as it grows (#179). An MCP call is one request and one
//! answer, so a stream to the AI is a series of short reads: `follow` runs a
//! command such as `journalctl -u nginx -f` and answers with an id at once,
//! `read_follow` gives what's new since the last read, and `stop_follow`
//! ends it.
//!
//! - **Allowed once.** A follow is asked for as `run_command` is
//!   (AiState::admit): the same settings and decision, the same card (which
//!   says it keeps running), the same log and limits. Reads ask nothing:
//!   they're the command you allowed. Not as root: a follow runs as the
//!   server tab's login.
//! - **On a channel of its own** (exec::follow): no terminal, nothing on
//!   stdin, PAGER=cat, and KILL and a close on any stop. Its time limit is
//!   LIFE, not a command's; it has no output cap, as its ring bounds it.
//! - **Bounded.** Its output goes into a ring of RING bytes, keeping the
//!   newest (from the start of a line); a read says how much was dropped
//!   since the last. A read gives at most READ_MAX, waits at most READ_WAIT
//!   for something new, and only one read of a follow waits at a time.
//! - **Filtered whole lines at a time.** A read gives whole lines: a line
//!   still coming is held back until a later read finds it ended, or one
//!   hears nothing new for READ_WAIT (unless it may be a key's first line),
//!   or the follow ends. So a secret split between two pieces of output is
//!   filtered whole. A secret of several lines (a private key block, the
//!   same in base64, a PuTTY key: `Block`) is held back from its first line
//!   until it ends, and filtered whole; held past RING, or when the follow
//!   ends, it's given as [hidden by Submarine], never in clear. Then it's
//!   marked as the server's (boundary.rs), after Submarine's own lines.
//! - **Ends** with stop_follow, Stop in the AI panel, UNREAD without a read,
//!   LIFE, the command ending, or access ending (the server unshared, the AI
//!   tab closed, AI tools turned off): while it runs it's a running request,
//!   whose stop signal every end of access sets (cancel_where).
//! - **Ids** are 128 random bits (boundary::fresh_id), looked up only in the
//!   asking AI tab's own list. An AI tab has at most MAX_FOLLOWS open,
//!   counting those that ended and weren't read to the end; after the last
//!   read, the id is gone, and an ended one nobody reads to the end goes
//!   UNREAD after it ended (logged).
//! - **Logged** once as it's allowed and once as it ends; the AI panel
//!   shows it while it runs, with Stop.

use super::boundary::ServerText;
use super::*;
use regex::bytes::Regex as BytesRegex;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::sync::Notify;

/// What a follow keeps between reads, at most: the newest output.
pub(super) static RING: usize = 256 * 1024;
/// Follows one AI tab may have open at once.
pub(super) static MAX_FOLLOWS: usize = 4;
/// The longest a follow runs.
pub(super) static LIFE: Duration = Duration::from_secs(2 * 60 * 60);
/// A follow nobody reads for this long is stopped.
static UNREAD: Duration = Duration::from_secs(30 * 60);
/// How long a read waits for something new.
static READ_WAIT: Duration = Duration::from_secs(5);
/// What one read gives at most: as much as a command's output.
static READ_MAX: usize = AI_HEAD + AI_TAIL;
/// How long stop_follow waits for the follow to end on the server.
static STOP_WAIT: Duration = Duration::from_secs(15);
/// How often the panel hears that a running follow saw more.
static PANEL_TICK: Duration = Duration::from_secs(5);
/// The longest id taken (they're 32 hex digits).
static MAX_ID: usize = 64;

/// Runs a follow on a session's connection, its output going to the feed as
/// it comes: the real one over SshState, or a test's. `(session_id,
/// command, limit, stop, feed)`; see exec::follow.
pub(super) type Follower = Arc<dyn Fn(String, String, Duration, watch::Receiver<bool>, Arc<Feed>) -> ExecFuture + Send + Sync>;

/// The app's: over SshState's connections.
pub(super) fn app_follower(app: tauri::AppHandle) -> Follower {
    Arc::new(move |session_id, command, limit, stop, feed| {
        let app = app.clone();
        Box::pin(async move {
            use tauri::Manager;
            let state = app.state::<crate::ssh_manager::SshState>();
            let handle = state.connections.lock().await.get(&session_id).cloned();
            let handle = handle.ok_or("That server's tab isn't connected right now.")?;
            exec::follow(&handle, &command, limit, stop, move |data| feed.push(data)).await
        })
    })
}

/// A running follow in the panel.
#[derive(Serialize, Clone, Default, Debug)]
pub(super) struct FollowView {
    bytes_seen: u64,
}

/// An ended follow in the panel's history.
#[derive(Serialize, Clone, Debug)]
pub(super) struct FollowDone {
    /// How it ended, for you.
    ended: String,
    bytes_seen: u64,
}

/// Who stopped it, when it was stopped on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    Ai,
    You,
    Unread,
}

/// How a follow ended.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Ending {
    Exited { code: Option<u32>, signal: Option<String> },
    ByAi,
    ByYou,
    Unread,
    LifeCap,
    AccessEnded,
    Failed(String),
}

impl Ending {
    fn for_ai(&self) -> String {
        match self {
            Ending::Exited { code: Some(c), .. } => format!("exit code {c}"),
            Ending::Exited { code: None, signal: Some(s) } => format!("ended by signal {s}"),
            Ending::Exited { .. } => "exit code unknown".into(),
            Ending::ByAi => "stopped by you (stop_follow)".into(),
            Ending::ByYou => "stopped by the user in Submarine".into(),
            Ending::Unread => format!("stopped after {} minutes with no reads", UNREAD.as_secs() / 60),
            Ending::LifeCap => format!("stopped after {} hours, the most a follow runs", LIFE.as_secs() / 3600),
            Ending::AccessEnded => "stopped: the server isn't shared any more".into(),
            Ending::Failed(e) => format!("couldn't run it: {e}"),
        }
    }

    fn for_you(&self) -> String {
        match self {
            Ending::ByAi => "stopped by the AI".into(),
            Ending::ByYou => "stopped by you".into(),
            Ending::AccessEnded => "stopped: access ended".into(),
            other => other.for_ai(),
        }
    }

    /// The panel's outcome.
    fn outcome(&self) -> &'static str {
        match self {
            Ending::Exited { .. } => "done",
            Ending::Failed(_) => "failed",
            _ => "stopped",
        }
    }
}

/// The newest RING bytes of a follow's output not read yet, in a buffer of
/// a fixed size that's wiped when dropped.
struct Ring {
    buf: Zeroizing<Vec<u8>>,
    head: usize,
    len: usize,
}

impl Ring {
    fn new() -> Ring {
        Ring { buf: Zeroizing::new(vec![0; RING]), head: 0, len: 0 }
    }

    fn at(&self, i: usize) -> u8 {
        self.buf[(self.head + i) % RING]
    }

    fn slices(&self) -> (&[u8], &[u8]) {
        let first = self.len.min(RING - self.head);
        (&self.buf[self.head..self.head + first], &self.buf[..self.len - first])
    }

    fn drop_front(&mut self, n: usize) {
        let n = n.min(self.len);
        self.head = (self.head + n) % RING;
        self.len -= n;
    }

    /// Add `data`, dropping the oldest to make room: to the end of the line
    /// it cut into, where one ends, so what's kept starts with a whole line.
    /// Returns how many bytes were dropped.
    fn push(&mut self, data: &[u8]) -> u64 {
        let mut dropped = 0;
        let mut mid_line = false;
        let data = if data.len() > RING {
            dropped += self.len + data.len() - RING;
            mid_line = data[data.len() - RING - 1] != b'\n';
            self.head = 0;
            self.len = 0;
            &data[data.len() - RING..]
        } else {
            let over = (self.len + data.len()).saturating_sub(RING);
            if over > 0 {
                mid_line = self.at(over - 1) != b'\n';
                self.drop_front(over);
                dropped += over;
            }
            data
        };
        let tail = (self.head + self.len) % RING;
        let first = data.len().min(RING - tail);
        self.buf[tail..tail + first].copy_from_slice(&data[..first]);
        self.buf[..data.len() - first].copy_from_slice(&data[first..]);
        self.len += data.len();
        if mid_line {
            if let Some(i) = (0..self.len).find(|&i| self.at(i) == b'\n') {
                self.drop_front(i + 1);
                dropped += i + 1;
            }
        }
        dropped as u64
    }

    /// Up to `max` bytes from the front.
    fn take(&mut self, max: usize) -> Zeroizing<Vec<u8>> {
        let n = max.min(self.len);
        let (a, b) = self.slices();
        let mut out = Zeroizing::new(Vec::with_capacity(n));
        out.extend_from_slice(&a[..n.min(a.len())]);
        out.extend_from_slice(&b[..n - n.min(a.len())]);
        self.drop_front(n);
        out
    }

    /// It holds the end of a line.
    fn has_boundary(&self) -> bool {
        let (a, b) = self.slices();
        a.iter().chain(b).any(|c| matches!(c, b'\n' | b'\r'))
    }
}

struct FeedState {
    ring: Ring,
    /// Dropped since the last read.
    dropped: u64,
    /// Everything the command sent.
    seen: u64,
    /// What the AI was given, and how many secrets were hidden from it.
    given: u64,
    hidden: u64,
    /// The last read (the start counts as one).
    last_read: tokio::time::Instant,
    why: Option<Why>,
    end: Option<Ending>,
}

/// A follow's output on its way to the AI: filled as it comes, emptied by
/// reads. Locked only for a moment, never across an `.await`.
pub(super) struct Feed {
    state: std::sync::Mutex<FeedState>,
    /// Woken when something comes, or it ends.
    news: Notify,
}

/// What a read takes from the feed.
struct Taken {
    bytes: Zeroizing<Vec<u8>>,
    dropped: u64,
    end: Option<Ending>,
    /// More is left in the ring.
    more: bool,
}

impl Feed {
    pub(super) fn new() -> Feed {
        Feed {
            state: std::sync::Mutex::new(FeedState {
                ring: Ring::new(),
                dropped: 0,
                seen: 0,
                given: 0,
                hidden: 0,
                last_read: tokio::time::Instant::now(),
                why: None,
                end: None,
            }),
            news: Notify::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FeedState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Output as it comes.
    pub(super) fn push(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        {
            let mut st = self.lock();
            st.seen += data.len() as u64;
            let dropped = st.ring.push(data);
            st.dropped += dropped;
        }
        self.news.notify_one();
    }

    pub(super) fn seen(&self) -> u64 {
        self.lock().seen
    }

    fn ended(&self) -> bool {
        self.lock().end.is_some()
    }

    fn last_read(&self) -> tokio::time::Instant {
        self.lock().last_read
    }

    fn touch(&self) {
        self.lock().last_read = tokio::time::Instant::now();
    }

    /// Who stopped it: the first to say counts.
    fn stopped_by(&self, why: Why) {
        let mut st = self.lock();
        if st.end.is_none() && st.why.is_none() {
            st.why = Some(why);
        }
    }

    fn why(&self) -> Option<Why> {
        self.lock().why
    }

    /// It ended: the wait for its last read starts now.
    fn end(&self, ending: Ending) {
        let mut st = self.lock();
        st.end = Some(ending);
        st.last_read = tokio::time::Instant::now();
        drop(st);
        self.news.notify_one();
    }

    /// Something for a read: a whole line, a read's worth, or the end.
    fn ready(&self) -> bool {
        let st = self.lock();
        st.end.is_some() || st.ring.len >= READ_MAX || st.ring.has_boundary()
    }

    fn take(&self, max: usize) -> Taken {
        let mut st = self.lock();
        let bytes = st.ring.take(max);
        let dropped = std::mem::take(&mut st.dropped);
        st.last_read = tokio::time::Instant::now();
        Taken { bytes, dropped, end: st.end.clone(), more: st.ring.len > 0 }
    }

    fn count(&self, given: usize, hidden: usize) {
        let mut st = self.lock();
        st.given += given as u64;
        st.hidden += hidden as u64;
    }

    fn totals(&self) -> (u64, u64, u64) {
        let st = self.lock();
        (st.seen, st.given, st.hidden)
    }
}

/// What the reads of one follow carry from one to the next.
#[derive(Default)]
struct Reader {
    /// A line still coming, or a key block not finished yet, held back.
    /// Wiped when dropped.
    carry: Zeroizing<Vec<u8>>,
    /// A key block held back past RING was given hidden: the rest of it is
    /// hidden too, up to its end.
    hiding: Option<Block>,
}

/// A follow in its AI tab's list (Tab::follows).
#[derive(Clone)]
pub(super) struct Entry {
    id: String,
    /// Its request's number (the running list's, the log's).
    request: u64,
    server: String,
    feed: Arc<Feed>,
    /// One read at a time.
    reader: Arc<tokio::sync::Mutex<Reader>>,
}

/// The follows AI tab `tab_id` has open, for MAX_FOLLOWS: waiting for you,
/// running, or ended and not read to the end. One that's just ending may
/// count twice for a moment, never not at all.
pub(super) fn open_in(s: &Shared, tab_id: &str) -> usize {
    let waiting = s.pending.iter().filter(|p| p.view.tab_id == tab_id && p.view.follow).count();
    let running = s.running.iter().filter(|r| r.tab_id == tab_id && r.view.follow.is_some()).count();
    let ended = s.tabs.iter().find(|t| t.id == tab_id).map_or(0, |t| t.follows.iter().filter(|e| e.feed.ended()).count());
    waiting + running + ended
}

/// A running request as the panel shows it: a follow with what it's seen.
pub(super) fn running_view(r: &Running) -> RunningView {
    let mut v = r.view.clone();
    if let (Some(f), Some(feed)) = (v.follow.as_mut(), r.feed.as_ref()) {
        f.bytes_seen = feed.seen();
    }
    v
}

/// A secret that takes several lines: a private key block (PEM, OpenSSH,
/// PGP), the same in base64 (as Kubernetes secrets hold it), or a PuTTY key
/// file. A read never gives one in pieces: it's held back until it ends,
/// then filtered whole, or given hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Pem,
    Base64,
    Putty,
}

static PEM_BEGIN: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----").expect("key rule"));
static PEM_END: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"-----END [A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----").expect("key rule"));
/// "-----BEGIN" in base64, as filter.rs looks for it.
static B64_BEGIN: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"LS0tLS1CRUdJTi").expect("key rule"));
/// A wrapped line of it, as filter.rs takes one.
static B64_LINE: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"^[A-Za-z0-9+/]{16,}={0,2}\r?$").expect("key rule"));
static PUTTY_BEGIN: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"PuTTY-User-Key-File-").expect("key rule"));
static PUTTY_END: LazyLock<BytesRegex> = LazyLock::new(|| BytesRegex::new(r"Private-MAC:").expect("key rule"));

/// What the start of a block can look like, in part: a line still coming
/// that holds one of these, or ends with the start of one, isn't given
/// before its line ends, as it may be a block's first line.
static BLOCK_HINTS: [&[u8]; 3] = [b"-----", b"LS0t", b"PuTTY"];

fn line_end(text: &[u8], from: usize) -> Option<usize> {
    text[from..].iter().position(|c| *c == b'\n').map(|i| from + i + 1)
}

/// Where a block of `kind` ends in `text`, looking from `from` (the end of
/// its start, or 0 for the rest of one): after its END line's marker, its
/// `Private-MAC:` line, or (base64) before the first whole line that isn't
/// base64 after its first line (`first_line`: `from` is on it). None: not
/// in `text`.
fn block_end(kind: Block, text: &[u8], from: usize, first_line: bool) -> Option<usize> {
    match kind {
        Block::Pem => PEM_END.find_at(text, from).map(|m| m.end()),
        Block::Putty => line_end(text, PUTTY_END.find_at(text, from)?.end()),
        Block::Base64 => {
            let mut at = if first_line { line_end(text, from)? } else { from };
            loop {
                let next = line_end(text, at)?;
                if !B64_LINE.is_match(&text[at..next - 1]) {
                    return Some(at);
                }
                at = next;
            }
        }
    }
}

/// The first block in `text` from `from` that doesn't end in it: where it
/// starts, and what it is.
fn unfinished(text: &[u8], from: usize) -> Option<(usize, Block)> {
    let mut first: Option<(usize, Block)> = None;
    for (kind, start) in [(Block::Pem, &*PEM_BEGIN), (Block::Base64, &*B64_BEGIN), (Block::Putty, &*PUTTY_BEGIN)] {
        for m in start.find_iter(&text[from..]) {
            let at = from + m.start();
            if first.is_some_and(|(f, _)| f <= at) {
                break;
            }
            if block_end(kind, text, from + m.end(), kind == Block::Base64).is_none() {
                first = Some((at, kind));
                break;
            }
        }
    }
    first
}

/// A line still coming that may be (or become) a block's first line.
fn may_start_block(partial: &[u8]) -> bool {
    BLOCK_HINTS.iter().any(|h| partial.windows(h.len()).any(|w| w == *h) || (1..h.len()).any(|k| partial.ends_with(&h[..k])))
}

/// What a read gives, filtered, and what it holds back for the next.
struct Prepared {
    text: Zeroizing<String>,
    hidden: usize,
    carry: Zeroizing<Vec<u8>>,
    hiding: Option<Block>,
    /// A key block is held back, waiting for its end.
    holding: bool,
}

/// `buf` (what was held back, then what's new) as a read gives it:
/// everything before `cut` (whole lines, or all of it when `last` or after
/// a quiet wait), less any key block that doesn't end there, which is held
/// back from its first line; through the filter, a block whole. A block
/// held past RING, or when the follow has ended (`last`), is given as
/// [hidden by Submarine], and (past RING) the rest of it after it too
/// (`hiding`), never in clear.
fn prepare(buf: &[u8], mut cut: usize, last: bool, mut hiding: Option<Block>) -> Prepared {
    let mut hidden = 0;
    // The rest of a block given hidden before.
    let mut hide_to = 0;
    if let Some(kind) = hiding {
        match block_end(kind, &buf[..cut], 0, false) {
            Some(end) => {
                hide_to = end;
                hiding = None;
            }
            None => hide_to = cut,
        }
    }
    // A block that doesn't end in what's to be given: held back from the
    // start of its line.
    let mut holding = None;
    if hiding.is_none() {
        if let Some((start, kind)) = unfinished(&buf[..cut], hide_to) {
            cut = buf[..start].iter().rposition(|c| *c == b'\n').map_or(0, |i| i + 1).max(hide_to);
            holding = Some(kind);
        }
    }
    let mut hide_held = false;
    if let Some(kind) = holding {
        if last || buf.len() - cut > RING {
            hide_held = true;
            hiding = (!last).then_some(kind);
            holding = None;
        }
    } else if buf.len() - cut > RING {
        // A line longer than the ring: given as it is (shortened below).
        cut = buf.len();
        if hiding.is_some() {
            hide_to = cut;
        }
    }
    let mut text = Zeroizing::new(String::new());
    let hidden_part = |text: &mut String, part: &[u8]| {
        text.push_str(filter::HIDDEN);
        if part.ends_with(b"\n") {
            text.push('\n');
        }
    };
    if hide_to > 0 {
        hidden += 1;
        hidden_part(&mut text, &buf[..hide_to]);
    }
    let plain = Zeroizing::new(String::from_utf8_lossy(&buf[hide_to..cut]).into_owned());
    let f = filter::filter(&plain);
    text.push_str(&f.text);
    hidden += f.hidden;
    let carry = if hide_held {
        hidden += 1;
        hidden_part(&mut text, &buf[cut..]);
        Zeroizing::new(Vec::new())
    } else {
        Zeroizing::new(buf[cut..].to_vec())
    };
    if text.len() > READ_MAX {
        text = Zeroizing::new(shorten_for_ai(&text));
    }
    Prepared { text, hidden, carry, hiding, holding: holding.is_some() }
}

fn unknown_id() -> ToolResult {
    ToolResult::err("No follow with that id in this AI tab. It may have ended and been read to the end, or the id is wrong: call follow to start one.")
}

impl AiState {
    pub(super) fn set_follower(&self, follower: Follower) {
        *self.0.follower.lock().unwrap_or_else(|e| e.into_inner()) = Some(follower);
    }

    /// `follow`, `read_follow` and `stop_follow`, their arguments checked.
    pub(super) async fn follow_tool(&self, tab_id: &str, name: &str, args: &Value, cancel: watch::Receiver<bool>) -> ToolResult {
        if name == "follow" {
            if args.get("as_root").is_some_and(|v| !v.is_null() && v != &Value::Bool(false)) {
                return ToolResult::err("follow runs as the user's SSH login: as_root isn't offered for it.");
            }
            if args.get("timeout_seconds").is_some_and(|v| !v.is_null()) {
                return ToolResult::err("follow takes no timeout_seconds: it runs until it's stopped, at most 2 hours.");
            }
            let server = args.get("server").and_then(Value::as_str).unwrap_or("");
            let command = args.get("command").and_then(Value::as_str).unwrap_or("");
            return self.follow(tab_id, server, command, cancel).await;
        }
        let Some(id) = args.get("id").and_then(Value::as_str).filter(|id| !id.is_empty() && id.len() <= MAX_ID) else {
            return ToolResult::err("id must be a follow's id, as follow gave it.");
        };
        if name == "read_follow" {
            self.read_follow(tab_id, id, cancel).await
        } else {
            self.stop_follow(tab_id, id).await
        }
    }

    /// Ask for a follow as `run_command` asks (admit, the card, your
    /// answer), and once it's allowed start it and answer with its id.
    pub(super) async fn follow(&self, tab_id: &str, server: &str, command: &str, mut cancel: watch::Receiver<bool>) -> ToolResult {
        let Admitted { req, rx, auto, stop_tx, stop_rx, card: _ } = match self.admit(tab_id, server, command, None, false, None, Kind::Follow).await {
            Ok(a) => a,
            Err(refused) => return refused,
        };
        let approved_by = match (auto, rx) {
            (Some(by), _) => by,
            (None, Some(rx)) => match self.await_answer(&req, rx, &mut cancel, Asked::Nothing).await {
                Ok((by, _)) => by,
                Err(answer) => return answer,
            },
            (None, None) => return ToolResult::err("It stopped unexpectedly."),
        };
        self.log_allowed(&req, &approved_by, 1);
        self.start_follow(req, approved_by, stop_tx, stop_rx, &cancel)
    }

    /// Start an allowed follow: its feed and id in its AI tab's list, its
    /// run and the task that watches it, each a task of its own. Unless the
    /// AI withdrew it, or access ended, as it was allowed.
    fn start_follow(&self, req: Req, approved_by: String, stop_tx: watch::Sender<bool>, stop_rx: watch::Receiver<bool>, cancel: &watch::Receiver<bool>) -> ToolResult {
        let ran_without_asking = Some(approved_by).filter(|b| b != BY_YOU);
        let follower = self.0.follower.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let not_now = if *cancel.borrow() {
            Some((Ending::ByAi, "Stopped: the request was withdrawn.".to_string()))
        } else if follower.is_none() {
            Some((Ending::Failed("Following isn't available here.".into()), "Couldn't follow it: following isn't available here.".to_string()))
        } else {
            None
        };
        let id = boundary::fresh_id();
        let (follower, id) = match (not_now, follower, id) {
            (Some((ending, why)), _, _) => {
                self.finish(&req, ran_without_asking, &ending, None);
                return ToolResult::err(why);
            }
            (None, _, Err(())) => {
                let ending = Ending::Failed("Submarine couldn't draw an id for it.".into());
                self.finish(&req, ran_without_asking, &ending, None);
                return ToolResult::err("Couldn't follow it: Submarine couldn't draw an id for it.");
            }
            (None, Some(f), Ok(id)) => (f, id),
            (None, None, _) => return ToolResult::err("It stopped unexpectedly."),
        };
        let feed = Arc::new(Feed::new());
        // In its AI tab's list, and with its feed in the running list, only
        // while it's still running (nothing ended its access meanwhile).
        let placed = {
            let mut s = self.lock();
            let listed = match s.running.iter_mut().find(|r| r.view.id == req.id) {
                Some(r) => {
                    r.feed = Some(feed.clone());
                    true
                }
                None => false,
            };
            match s.tabs.iter_mut().find(|t| t.id == req.tab_id).filter(|_| listed) {
                Some(t) => {
                    t.follows.push(Entry {
                        id: id.clone(),
                        request: req.id,
                        server: req.server.clone(),
                        feed: feed.clone(),
                        reader: Arc::default(),
                    });
                    true
                }
                None => false,
            }
        };
        if !placed {
            self.finish(&req, ran_without_asking, &Ending::AccessEnded, None);
            return ToolResult::err("Stopped: the server isn't shared any more.");
        }
        let said = format!(
            "Following it on {}. Its id: {id}\nCall read_follow with this id for what's new (each read waits up to {} seconds), and stop_follow when you're done. \
             It stops by itself after {} minutes without a read, or after {} hours.",
            req.server,
            READ_WAIT.as_secs(),
            UNREAD.as_secs() / 60,
            LIFE.as_secs() / 3600
        );
        // Each a task of its own: exec::follow ends the command on the server
        // whatever happens to the call that started it.
        let task = tokio::spawn(follower(req.session_id.clone(), req.command.clone(), LIFE, stop_rx, feed.clone()));
        let me = self.clone();
        tokio::spawn(async move { me.watch_follow(req, id, ran_without_asking, feed, task, stop_tx).await });
        self.changed();
        ToolResult::ok(said)
    }

    /// While a follow runs: stopped after UNREAD without a read, the panel
    /// told every PANEL_TICK that it saw more; once it ends, how it ended
    /// goes to its feed (for the last read), the log and the history. Ended
    /// and not read to the end for UNREAD, it's taken off its AI tab's list,
    /// with what it still held.
    async fn watch_follow(
        &self,
        req: Req,
        id: String,
        ran_without_asking: Option<String>,
        feed: Arc<Feed>,
        mut task: tokio::task::JoinHandle<Result<exec::Exec, String>>,
        stop_tx: watch::Sender<bool>,
    ) {
        let mut stopping = false;
        let mut told = 0;
        let mut tick = tokio::time::interval(PANEL_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let joined = loop {
            let unread_at = feed.last_read() + UNREAD;
            tokio::select! {
                r = &mut task => break r,
                _ = tokio::time::sleep_until(unread_at), if !stopping => {
                    if feed.last_read() + UNREAD <= tokio::time::Instant::now() {
                        feed.stopped_by(Why::Unread);
                        let _ = stop_tx.send(true);
                        stopping = true;
                    }
                }
                _ = tick.tick() => {
                    let seen = feed.seen();
                    if seen != told {
                        told = seen;
                        self.changed();
                    }
                }
            }
        };
        let ending = match joined {
            Err(e) => Ending::Failed(format!("It stopped unexpectedly: {e}")),
            Ok(Err(e)) => Ending::Failed(e),
            Ok(Ok(out)) if out.timed_out => Ending::LifeCap,
            Ok(Ok(out)) if out.stopped => match feed.why() {
                Some(Why::Ai) => Ending::ByAi,
                Some(Why::You) => Ending::ByYou,
                Some(Why::Unread) => Ending::Unread,
                None => Ending::AccessEnded,
            },
            Ok(Ok(out)) => Ending::Exited { code: out.exit_status, signal: out.exit_signal },
        };
        // The feed first, then the running list: it counts toward
        // MAX_FOLLOWS all the while.
        feed.end(ending.clone());
        drop(stop_tx);
        self.finish(&req, ran_without_asking, &ending, Some(&feed));
        // Held only weakly from here: read to the end (or its AI tab gone),
        // its buffer goes at once.
        let feed = Arc::downgrade(&feed);
        loop {
            let Some(at) = feed.upgrade().map(|f| f.last_read() + UNREAD) else { return };
            tokio::time::sleep_until(at).await;
            let Some(f) = feed.upgrade() else { return };
            if f.last_read() + UNREAD > tokio::time::Instant::now() {
                continue;
            }
            let removed = {
                let mut s = self.lock();
                let tab = s.tabs.iter_mut().find(|t| t.id == req.tab_id);
                tab.is_some_and(|t| {
                    let before = t.follows.len();
                    t.follows.retain(|e| e.id != id);
                    before != t.follows.len()
                })
            };
            if removed {
                let mut e = req.entry("follow_removed");
                e.note = Some(format!("ended and not read to the end for {} minutes: what it still held was dropped", UNREAD.as_secs() / 60));
                self.audit(e);
                self.changed();
            }
            return;
        }
    }

    /// A follow's end: off the running list, its "finished" line in the
    /// log, and its row in the history.
    fn finish(&self, req: &Req, ran_without_asking: Option<String>, ending: &Ending, feed: Option<&Feed>) {
        self.lock().running.retain(|r| r.view.id != req.id);
        let (seen, given, hidden) = feed.map(Feed::totals).unwrap_or_default();
        let code = match ending {
            Ending::Exited { code, .. } => code.map(i64::from),
            _ => None,
        };
        let mut e = req.entry("finished");
        e.exit_code = code;
        e.bytes_returned = Some(given as usize);
        e.items_hidden = Some(hidden as usize);
        e.note = Some(format!("follow {}; {} bytes seen", ending.for_you(), seen));
        self.audit(e);
        let mut d = req.done(ending.outcome());
        d.view.ran_without_asking = ran_without_asking;
        d.view.exit_code = code;
        d.view.output_bytes = seen as usize;
        d.view.hidden = hidden as usize;
        d.view.timed_out = *ending == Ending::LifeCap;
        d.view.error = match ending {
            Ending::Failed(e) => Some(e.clone()),
            _ => None,
        };
        d.view.follow = Some(FollowDone { ended: ending.for_you(), bytes_seen: seen });
        self.remember(req.epoch, d);
        self.changed();
    }

    /// The follow `id` in AI tab `tab_id`'s own list.
    fn follow_of(&self, tab_id: &str, id: &str) -> Option<Entry> {
        self.lock().tabs.iter().find(|t| t.id == tab_id)?.follows.iter().find(|e| e.id == id).cloned()
    }

    fn remove_follow(&self, tab_id: &str, id: &str) {
        if let Some(t) = self.lock().tabs.iter_mut().find(|t| t.id == tab_id) {
            t.follows.retain(|e| e.id != id);
        }
    }

    /// `read_follow`: what's new since the last read, once there's a whole
    /// line, or it's ended, or READ_WAIT has passed.
    pub(super) async fn read_follow(&self, tab_id: &str, id: &str, mut cancel: watch::Receiver<bool>) -> ToolResult {
        let Some(Entry { feed, reader, server, .. }) = self.follow_of(tab_id, id) else { return unknown_id() };
        let Ok(mut reader) = reader.try_lock_owned() else {
            return ToolResult::err("Another read of this follow is still waiting. Wait for its answer first.");
        };
        feed.touch();
        let deadline = tokio::time::Instant::now() + READ_WAIT;
        let mut quiet = false;
        while !feed.ready() {
            tokio::select! {
                _ = feed.news.notified() => {}
                _ = tokio::time::sleep_until(deadline) => {
                    quiet = true;
                    break;
                }
                // Nothing is taken from it: the next read gets it.
                _ = withdrawal(&mut cancel) => return ToolResult::err("Cancelled: the read was withdrawn."),
            }
        }
        self.give(tab_id, id, &feed, &mut reader, &server, quiet).await
    }

    /// `stop_follow`: ended on the server, then what's left and how it
    /// ended, as the last read.
    pub(super) async fn stop_follow(&self, tab_id: &str, id: &str) -> ToolResult {
        let Some(Entry { feed, reader, server, request, .. }) = self.follow_of(tab_id, id) else { return unknown_id() };
        if !feed.ended() {
            feed.stopped_by(Why::Ai);
            let s = self.lock();
            if let Some(r) = s.running.iter().find(|r| r.view.id == request) {
                let _ = r.stop.send(true);
            }
        }
        let later = "Stopping it. read_follow with its id gives what's left, and how it ended.";
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        let Ok(mut reader) = tokio::time::timeout_at(deadline, reader.lock_owned()).await else { return ToolResult::ok(later) };
        while !feed.ended() {
            tokio::select! {
                _ = feed.news.notified() => {}
                _ = tokio::time::sleep_until(deadline) => return ToolResult::ok(later),
            }
        }
        self.give(tab_id, id, &feed, &mut reader, &server, true).await
    }

    /// The next read's worth: whole lines (a line still coming is held
    /// back, unless `quiet`: nothing new came for a whole wait), filtered,
    /// then marked as the server's after Submarine's own lines. The last
    /// read of an ended follow takes it off the list.
    async fn give(&self, tab_id: &str, id: &str, feed: &Feed, reader: &mut Reader, server: &str, quiet: bool) -> ToolResult {
        let taken = feed.take(READ_MAX);
        let mut buf = Zeroizing::new(Vec::with_capacity(reader.carry.len() + taken.bytes.len() + 1));
        if !reader.carry.is_empty() {
            buf.extend_from_slice(&reader.carry);
            // What came after it was dropped: it's a line of its own.
            if taken.dropped > 0 {
                buf.push(b'\n');
            }
        }
        buf.extend_from_slice(&taken.bytes);
        let last = taken.end.is_some() && !taken.more;
        // After a wait with nothing new, a line still coming is given too,
        // unless it may be the first line of a key block.
        let quiet = quiet && taken.bytes.is_empty() && !may_start_block(&buf);
        let cut = if last || quiet { buf.len() } else { buf.iter().rposition(|c| matches!(c, b'\n' | b'\r')).map_or(0, |i| i + 1) };
        let hiding = reader.hiding;
        let read = tokio::task::spawn_blocking(move || prepare(&buf, cut, last, hiding));
        let Ok(mut screened) = read.await else {
            return ToolResult::err("Submarine couldn't filter the output, so it isn't given.");
        };
        reader.carry = std::mem::take(&mut screened.carry);
        reader.hiding = screened.hiding;
        feed.count(screened.text.len(), screened.hidden);
        if last {
            self.remove_follow(tab_id, id);
        }
        let mut head = match &taken.end {
            Some(e) if last => format!("Ended: {}. That was the last of it: this follow's id no longer works.\n", e.for_ai()),
            Some(e) => format!("Ended: {}. More of its output is left: read again.\n", e.for_ai()),
            None if taken.more => "Still running. More is waiting: read again.\n".to_string(),
            None => "Still running.\n".to_string(),
        };
        if taken.dropped > 0 {
            head.push_str(&format!(
                "{} bytes were dropped since your last read (the buffer keeps the newest {} KB).\n",
                taken.dropped,
                RING / 1024
            ));
        }
        if screened.hidden > 0 {
            head.push_str(&format!(
                "Submarine hid {} item{} that looked like secrets (shown as {}).\n",
                screened.hidden,
                if screened.hidden == 1 { "" } else { "s" },
                filter::HIDDEN
            ));
        }
        if screened.holding {
            head.push_str("Output that looks like a private key is held back until it ends; then it's given filtered.\n");
        }
        if screened.text.is_empty() && taken.end.is_none() {
            head.push_str("Nothing new.");
        }
        let server_text = (!screened.text.is_empty()).then(|| ServerText { server: server.to_string(), text: screened.text.to_string() });
        ToolResult { text: head.trim_end().to_string(), is_error: false, server_text }
    }

    /// Stop in the AI panel, on the running follow `request`.
    pub(super) fn stop_follow_by_you(&self, request: u64) -> Result<(), String> {
        let s = self.lock();
        let r = s.running.iter().find(|r| r.view.id == request && r.view.follow.is_some()).ok_or("That follow isn't running any more.")?;
        if let Some(feed) = &r.feed {
            feed.stopped_by(Why::You);
        }
        let _ = r.stop.send(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests;

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks("lib.rs", include_str!("../lib.rs"), &[("run", "ai::ai_stop_follow", 1)]);
    }
}

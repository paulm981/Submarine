//! Tests only: a network link with a one-way delay, a bottleneck rate and a
//! buffer, shared by the connections dialled over it, for timing a jump host
//! login on a paused clock (`#[tokio::test(start_paused = true)]`): what's
//! measured is queuing and delay, the part sharing a login changes, not the
//! CPU time of the test machine.
//!
//! Each direction has its own bottleneck. It sends at `rate`, from a buffer of
//! `buffer` bytes that is either one queue for every connection (`Fifo`: a
//! home router or a VPN, where a flood fills the queue in front of everyone)
//! or a queue per connection served in turn (`Fair`: fq_codel, where a flow
//! of its own isn't held up by another's). A full buffer makes the writer
//! wait, as a full socket buffer does. What leaves the bottleneck arrives
//! `delay` later. `dials` counts the connections made, `cut` breaks them all.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Queuing {
    Fifo,
    Fair,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkSpec {
    /// One way.
    pub delay: Duration,
    /// Bytes a second, each way.
    pub rate: u64,
    /// The bottleneck's buffer: in all (`Fifo`), or per connection (`Fair`).
    pub buffer: usize,
    pub queuing: Queuing,
}

impl LinkSpec {
    /// `mbit` megabits a second, `rtt` there and back.
    pub(crate) fn new(mbit: u64, rtt: Duration, buffer: usize, queuing: Queuing) -> Self {
        LinkSpec { delay: rtt / 2, rate: mbit * 1_000_000 / 8, buffer, queuing }
    }
}

/// What the bottleneck sends in one go: at most this much of one connection.
const QUANTUM: usize = 4 * 1024;
/// How far ahead of the clock the bottleneck takes bytes from its buffer: the
/// clock moves in whole milliseconds.
const LOOKAHEAD: Duration = Duration::from_millis(1);

/// Bytes on their way to one end of one connection.
#[derive(Default)]
struct Inbox {
    data: VecDeque<u8>,
    eof: bool,
    reader: Option<Waker>,
}

#[derive(Default)]
struct FlowQueue {
    /// Waiting at the bottleneck; an empty segment is the writer's end.
    segments: VecDeque<Vec<u8>>,
    bytes: usize,
    writer: Option<Waker>,
}

struct Direction {
    spec: LinkSpec,
    state: Mutex<DirState>,
    /// Bytes came into the buffer.
    arrived: Notify,
    /// Bytes went onto the wire (for the delivery task).
    sent: Notify,
}

#[derive(Default)]
struct DirState {
    flows: Vec<FlowQueue>,
    inboxes: Vec<Arc<Mutex<Inbox>>>,
    /// `Fifo`: the flow of each segment, in the order they came in.
    order: VecDeque<usize>,
    total: usize,
    /// `Fair`: the next flow to serve.
    turn: usize,
    /// When the bottleneck is free again.
    free_at: Option<Instant>,
    /// On the wire: (arrives at, flow, bytes; empty = the writer's end).
    wire: VecDeque<(Instant, usize, Vec<u8>)>,
    /// Connections that were cut.
    broken: Vec<bool>,
    /// `Fifo`: writers that found the buffer full, in the order they came:
    /// room goes to them in turn, as packets reach a router one after
    /// another (none can push in front of one that came first).
    waiting: VecDeque<usize>,
}

impl Direction {
    fn new(spec: LinkSpec) -> Arc<Self> {
        let d = Arc::new(Direction { spec, state: Mutex::new(DirState::default()), arrived: Notify::new(), sent: Notify::new() });
        tokio::spawn(Arc::clone(&d).bottleneck());
        tokio::spawn(Arc::clone(&d).deliver());
        d
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DirState> {
        self.state.lock().unwrap()
    }

    fn room(&self, st: &DirState, flow: usize) -> usize {
        match self.spec.queuing {
            Queuing::Fifo => self.spec.buffer.saturating_sub(st.total),
            Queuing::Fair => self.spec.buffer.saturating_sub(st.flows[flow].bytes),
        }
    }

    /// The next bytes to send: (flow, bytes).
    fn take(&self, st: &mut DirState) -> Option<(usize, Vec<u8>)> {
        let flow = match self.spec.queuing {
            Queuing::Fifo => *st.order.front()?,
            Queuing::Fair => {
                let n = st.flows.len();
                let at = (0..n).map(|i| (st.turn + i) % n).find(|&f| !st.flows[f].segments.is_empty())?;
                st.turn = (at + 1) % n;
                at
            }
        };
        let q = &mut st.flows[flow];
        let seg = q.segments.front_mut()?;
        let whole = seg.len() <= QUANTUM;
        let bytes = if whole {
            q.segments.pop_front().unwrap_or_default()
        } else {
            let rest = seg.split_off(QUANTUM);
            std::mem::replace(seg, rest)
        };
        q.bytes -= bytes.len();
        st.total -= bytes.len();
        // Room was made: for the writer first in line.
        let next = match self.spec.queuing {
            Queuing::Fifo => st.waiting.front().copied(),
            Queuing::Fair => Some(flow),
        };
        if let Some(w) = next.and_then(|f| st.flows[f].writer.take()) {
            w.wake();
        }
        if whole && self.spec.queuing == Queuing::Fifo {
            st.order.pop_front();
        }
        Some((flow, bytes))
    }

    async fn bottleneck(self: Arc<Self>) {
        loop {
            let next = {
                let mut st = self.lock();
                let now = Instant::now();
                let mut took = false;
                loop {
                    let start = st.free_at.map_or(now, |f| f.max(now));
                    if start > now + LOOKAHEAD {
                        break;
                    }
                    let Some((flow, bytes)) = self.take(&mut st) else { break };
                    let done = start + Duration::from_nanos(bytes.len() as u64 * 1_000_000_000 / self.spec.rate);
                    st.free_at = Some(done);
                    st.wire.push_back((done + self.spec.delay, flow, bytes));
                    took = true;
                }
                if took {
                    self.sent.notify_one();
                }
                let queued = st.flows.iter().any(|f| !f.segments.is_empty());
                queued.then(|| st.free_at.unwrap_or(now))
            };
            match next {
                Some(at) => tokio::time::sleep_until(at.checked_sub(LOOKAHEAD).unwrap_or(at)).await,
                None => self.arrived.notified().await,
            }
        }
    }

    async fn deliver(self: Arc<Self>) {
        loop {
            let next = {
                let mut st = self.lock();
                let now = Instant::now();
                while st.wire.front().is_some_and(|(at, _, _)| *at <= now) {
                    let (_, flow, bytes) = st.wire.pop_front().unwrap();
                    if st.broken[flow] {
                        continue;
                    }
                    let mut inbox = st.inboxes[flow].lock().unwrap();
                    if bytes.is_empty() {
                        inbox.eof = true;
                    } else {
                        inbox.data.extend(bytes);
                    }
                    if let Some(w) = inbox.reader.take() {
                        w.wake();
                    }
                }
                st.wire.front().map(|(at, _, _)| *at)
            };
            match next {
                Some(at) => tokio::time::sleep_until(at).await,
                None => self.sent.notified().await,
            }
        }
    }

    fn add_flow(&self) -> (usize, Arc<Mutex<Inbox>>) {
        let mut st = self.lock();
        st.flows.push(FlowQueue::default());
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        st.inboxes.push(Arc::clone(&inbox));
        st.broken.push(false);
        (st.flows.len() - 1, inbox)
    }

    fn write(&self, flow: usize, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let mut st = self.lock();
        if st.broken[flow] {
            return Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()));
        }
        let room = self.room(&st, flow);
        let fifo = self.spec.queuing == Queuing::Fifo;
        let behind_another = fifo && st.waiting.front().is_some_and(|&f| f != flow);
        if room == 0 || behind_another {
            if fifo && !st.waiting.contains(&flow) {
                st.waiting.push_back(flow);
            }
            st.flows[flow].writer = Some(cx.waker().clone());
            return Poll::Pending;
        }
        if fifo && st.waiting.front() == Some(&flow) {
            st.waiting.pop_front();
        }
        let n = room.min(buf.len());
        self.push(&mut st, flow, buf[..n].to_vec());
        // Room left over: the next in line.
        if fifo && self.room(&st, flow) > 0 {
            if let Some(w) = st.waiting.front().copied().and_then(|f| st.flows[f].writer.take()) {
                w.wake();
            }
        }
        drop(st);
        self.arrived.notify_one();
        Poll::Ready(Ok(n))
    }

    fn push(&self, st: &mut DirState, flow: usize, bytes: Vec<u8>) {
        let n = bytes.len();
        let q = &mut st.flows[flow];
        q.bytes += n;
        q.segments.push_back(bytes);
        st.total += n;
        if self.spec.queuing == Queuing::Fifo {
            // An empty segment (the end) takes its place in the order too.
            st.order.push_back(flow);
        }
    }

    /// The writer of `flow` is done: its reader sees the end after the rest.
    fn close(&self, flow: usize) {
        let mut st = self.lock();
        // It writes no more: its place in line goes to the next.
        if let Some(at) = st.waiting.iter().position(|&f| f == flow) {
            st.waiting.remove(at);
            if let Some(w) = st.waiting.front().copied().and_then(|f| st.flows[f].writer.take()) {
                w.wake();
            }
        }
        self.push(&mut st, flow, Vec::new());
        drop(st);
        self.arrived.notify_one();
    }

    /// Every connection so far breaks: what's on its way is lost, its
    /// readers see the end and its writers an error. Later ones work.
    fn cut(&self, only: Option<usize>) {
        let mut st = self.lock();
        let st = &mut *st;
        for flow in 0..st.flows.len() {
            if only.is_some_and(|f| f != flow) {
                continue;
            }
            st.broken[flow] = true;
            let q = &mut st.flows[flow];
            st.total -= q.bytes;
            q.bytes = 0;
            q.segments.clear();
            if let Some(w) = q.writer.take() {
                w.wake();
            }
            let mut inbox = st.inboxes[flow].lock().unwrap();
            inbox.eof = true;
            inbox.data.clear();
            if let Some(w) = inbox.reader.take() {
                w.wake();
            }
        }
        let broken = std::mem::take(&mut st.broken);
        st.order.retain(|&f| !broken[f]);
        st.waiting.retain(|&f| !broken[f]);
        st.broken = broken;
        if let Some(w) = st.waiting.front().copied().and_then(|f| st.flows[f].writer.take()) {
            w.wake();
        }
    }
}

/// A link connections are dialled over.
pub(crate) struct ShapedLink {
    /// Server to client.
    down: Arc<Direction>,
    /// Client to server.
    up: Arc<Direction>,
    dials: AtomicUsize,
}

impl ShapedLink {
    /// Needs a tokio runtime: each direction runs two tasks.
    pub(crate) fn new(spec: LinkSpec) -> Arc<Self> {
        Arc::new(ShapedLink { down: Direction::new(spec), up: Direction::new(spec), dials: AtomicUsize::new(0) })
    }

    /// A new connection over the link: (the client's end, the server's end).
    pub(crate) fn dial(&self) -> (End, End) {
        self.dials.fetch_add(1, Ordering::SeqCst);
        let (up_flow, server_inbox) = self.up.add_flow();
        let (down_flow, client_inbox) = self.down.add_flow();
        assert_eq!(up_flow, down_flow);
        let client = End { send: Arc::clone(&self.up), flow: up_flow, inbox: client_inbox, closed: false };
        let server = End { send: Arc::clone(&self.down), flow: down_flow, inbox: server_inbox, closed: false };
        (client, server)
    }

    /// Connections dialled so far.
    pub(crate) fn dials(&self) -> usize {
        self.dials.load(Ordering::SeqCst)
    }

    /// Every connection over the link breaks at once.
    pub(crate) fn cut(&self) {
        self.up.cut(None);
        self.down.cut(None);
    }

    /// The `n`th connection dialled (from 0) breaks; the others carry on.
    pub(crate) fn cut_one(&self, n: usize) {
        self.up.cut(Some(n));
        self.down.cut(Some(n));
    }
}

/// One end of a connection over a `ShapedLink`.
pub(crate) struct End {
    send: Arc<Direction>,
    flow: usize,
    inbox: Arc<Mutex<Inbox>>,
    closed: bool,
}

impl AsyncRead for End {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let mut inbox = self.inbox.lock().unwrap();
        if inbox.data.is_empty() {
            if inbox.eof {
                return Poll::Ready(Ok(()));
            }
            inbox.reader = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let n = buf.remaining().min(inbox.data.len());
        let (a, b) = inbox.data.as_slices();
        let from_a = n.min(a.len());
        buf.put_slice(&a[..from_a]);
        buf.put_slice(&b[..n - from_a]);
        inbox.data.drain(..n);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for End {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if self.closed {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
        }
        self.send.write(self.flow, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if !self.closed {
            self.closed = true;
            self.send.close(self.flow);
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for End {
    fn drop(&mut self) {
        if !self.closed {
            self.closed = true;
            self.send.close(self.flow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A byte takes the delay; a megabyte takes its size at the rate too.
    #[tokio::test(start_paused = true)]
    async fn the_link_has_its_delay_and_rate() {
        let link = ShapedLink::new(LinkSpec::new(80, Duration::from_millis(40), 4 << 20, Queuing::Fifo));
        let (mut c, mut s) = link.dial();
        let t = Instant::now();
        c.write_all(b"x").await.unwrap();
        let mut one = [0u8; 1];
        s.read_exact(&mut one).await.unwrap();
        let took = t.elapsed();
        assert!(took >= Duration::from_millis(20) && took <= Duration::from_millis(22), "{took:?}");

        // 1 MB at 10 MB/s: 100 ms, plus the delay.
        let t = Instant::now();
        let reader = tokio::spawn(async move {
            let mut buf = vec![0u8; 1_000_000];
            c.read_exact(&mut buf).await.unwrap();
            Instant::now()
        });
        s.write_all(&vec![7u8; 1_000_000]).await.unwrap();
        let took = reader.await.unwrap() - t;
        assert!(took >= Duration::from_millis(119) && took <= Duration::from_millis(123), "{took:?}");
        assert_eq!(link.dials(), 1);
    }

    /// A FIFO link holds a small write behind a flood on another connection;
    /// a fair one doesn't.
    #[tokio::test(start_paused = true)]
    async fn fifo_queues_behind_a_flood_and_fair_does_not() {
        for (queuing, slow) in [(Queuing::Fifo, true), (Queuing::Fair, false)] {
            let link = ShapedLink::new(LinkSpec::new(8, Duration::from_millis(20), 1 << 20, queuing));
            let (mut flood_c, flood_s) = link.dial();
            let (mut small_c, mut small_s) = link.dial();
            let mut flood_s = flood_s;
            // 1 MB at 1 MB/s fills the buffer: a second of queue.
            flood_s.write_all(&vec![0u8; 1 << 20]).await.unwrap();
            let t = Instant::now();
            small_s.write_all(b"k").await.unwrap();
            let mut one = [0u8; 1];
            small_c.read_exact(&mut one).await.unwrap();
            let took = t.elapsed();
            if slow {
                assert!(took >= Duration::from_millis(900), "{queuing:?}: {took:?}");
            } else {
                assert!(took <= Duration::from_millis(20), "{queuing:?}: {took:?}");
            }
            drop(flood_c.read(&mut [0u8; 16]).await);
        }
    }

    /// A closed end is an EOF at the other once what it wrote has arrived; a
    /// cut is an EOF for every connection at once.
    #[tokio::test(start_paused = true)]
    async fn ends_and_cuts_are_seen() {
        let link = ShapedLink::new(LinkSpec::new(100, Duration::from_millis(10), 1 << 20, Queuing::Fair));
        let (mut c, mut s) = link.dial();
        c.write_all(b"bye").await.unwrap();
        c.shutdown().await.unwrap();
        let mut got = Vec::new();
        s.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"bye");

        let (mut c2, _s2) = link.dial();
        link.cut();
        let mut buf = [0u8; 4];
        assert_eq!(c2.read(&mut buf).await.unwrap(), 0);
        assert!(c2.write_all(b"x").await.is_err());
        // A connection dialled after the cut works.
        let (mut c3, mut s3) = link.dial();
        c3.write_all(b"new").await.unwrap();
        s3.read_exact(&mut buf[..3]).await.unwrap();
        assert_eq!(&buf[..3], b"new");
    }
}

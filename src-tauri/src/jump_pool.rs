//! One login per jump host, shared by the SSH terminal tabs that go through
//! it (#237).
//!
//! Opening a folder of servers behind one jump host used to log in to the
//! jump host once per tab: one password or code prompt each, and as many
//! logins as tabs on the bastion. Here a tab whose route reaches a jump host
//! that another tab has already logged in to, the same way, as the same user,
//! with the same credentials, opens its channel on that login instead.
//!
//! What may share:
//! - SSH terminal tabs only (`hop_policy::RouteQueues::shared`): RDP, VNC,
//!   telnet and FTP through a jump host, Monitoring, and a tab's `::sftp`,
//!   `::fwd` and `::term-` connections keep logins of their own: their bulk
//!   traffic would sit in front of a terminal's typing.
//! - A hop whose user is asked for at login (#54) isn't shared: who logs in
//!   isn't known until then.
//! - Only an identical login: `Key` holds the address, the user, the saved
//!   jump host (or ~/.ssh/config hop) it was made for, the exact connection
//!   of the hop before it, the profile (`epoch`) and a salted digest of the
//!   credentials and the hop's ~/.ssh/config. Nothing secret is kept: the
//!   digest's salt is random for each run and lives in memory only.
//!
//! How it lives:
//! - A login is made by the first tab that needs it (`Acquired::Create`); the
//!   others wait for it (`Pool::acquire`), and share it once it's ready, or
//!   fail with it (a wrong password fails them all, as an auth error, so they
//!   don't each ask again), or take over when the first tab is closed.
//! - Each tab holds a `Lease` on each login of its route, kept with its jump
//!   route (`ssh_manager::JumpRoute`) and dropped with it. At most `CAP` per
//!   login: the next tab makes a second login, once the first is ready, and
//!   reuses a password typed for the first.
//! - The last lease gone, the login stays `LINGER` for a tab that reconnects,
//!   then closes. A login that dropped is replaced by the next tab that comes.
//! - A shared login gets a smaller SSH window (`tune`): one tab's flood can
//!   then hold up another tab's echo on the same connection by at most
//!   `WINDOW` at the link's speed (42 ms at 100 Mbit/s), and a FIFO link by
//!   less than separate logins would (ssh_manager/fork_tests/jump_share.rs).
//!
//! Settings › Servers & sessions turns it off (`jump_share.json`). The pool
//! is emptied when the profile closes (`clear`, from
//! `SshState::forget_fork_sessions`), and a deleted jump host's logins take no
//! more tabs (`forget_saved`, from jump_gone.rs).

use russh::client::{Handle, Handler};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::watch;

/// The most tabs (and hops chained behind) on one login.
pub(crate) const CAP: usize = 16;
/// How long a login no tab uses any more stays open.
pub(crate) const LINGER: Duration = Duration::from_secs(30);
/// A shared login's SSH window and largest packet (`tune`).
pub(crate) const WINDOW: u32 = 512 * 1024;
pub(crate) const MAX_PACKET: u32 = 32 * 1024;

/// A shared login's client config: the window that bounds what one tab can
/// put in front of another's echo on the connection.
pub(crate) fn tune(mut config: russh::client::Config) -> russh::client::Config {
    config.window_size = WINDOW;
    config.maximum_packet_size = MAX_PACKET;
    config
}

/// The app's pool: the jump host logins of SSH terminal tabs.
pub(crate) static POOL: LazyLock<Arc<Pool<crate::ssh_manager::ClientHandler>>> = LazyLock::new(Pool::new);

/// A login every tab on it shares.
pub(crate) type Shared<H> = Arc<Handle<H>>;
/// The leases a tab's route holds (`ssh_manager::JumpRoute`).
pub type Leases = Vec<Lease<crate::ssh_manager::ClientHandler>>;

/// How a hop is reached.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Via {
    /// Dialled from here.
    Direct,
    /// Through this ProxyCommand.
    Command(String),
    /// Through the pool's login with this id: the exact connection of the hop
    /// before it, so `10.0.0.5` behind one gateway login never matches
    /// `10.0.0.5` behind another.
    Through(u64),
}

/// Which logins are the same login.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    epoch: u64,
    via: Via,
    /// `hop_policy::queue_key`.
    addr: String,
    user: String,
    /// The saved jump host (`jump<id>`) or ~/.ssh/config hop it was made for.
    login: String,
    /// `digest` of the credentials and the hop's config.
    cred: [u8; 32],
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The digest stays out of logs.
        write!(f, "{}@{} ({}, {:?}, epoch {})", self.user, self.addr, self.login, self.via, self.epoch)
    }
}

impl Key {
    /// `epoch` is set by the pool when the key is used (`Pool::acquire`).
    pub(crate) fn new(via: Via, addr: String, user: String, login: String, cred: [u8; 32]) -> Self {
        Key { epoch: 0, via, addr, user, login, cred }
    }
}

/// A digest of `parts` with this run's own random salt, so the key can tell
/// two credentials apart without holding either. Each part is length-prefixed,
/// so moving bytes from one part to the next changes it.
pub(crate) fn digest(parts: &[&[u8]]) -> [u8; 32] {
    use rand::Rng;
    use sha2::{Digest, Sha256};
    static SALT: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    let salt = SALT.get_or_init(|| {
        let mut b = [0u8; 32];
        rand::rng().fill_bytes(&mut b);
        b
    });
    let mut h = Sha256::new();
    h.update(b"submarine-jump-login\0");
    h.update(salt);
    for part in parts {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part);
    }
    h.finalize().into()
}

/// How a login being made ended, for the tabs waiting on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Ready,
    /// Its tab was closed or reconnected first: a waiting tab makes it.
    Abandoned,
    /// It failed. `auth`: refused or cancelled, so the waiting tabs stop too
    /// instead of asking again; `retry`: the network, worth trying again.
    Failed { auth: bool, retry: bool },
}

/// Why a tab didn't get a login to share or make.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Waited {
    /// The login it waited on failed.
    Failed { auth: bool, retry: bool },
    /// The tab itself was closed or reconnected while it waited.
    Cancelled,
}

enum State<H: Handler + 'static> {
    Creating(watch::Receiver<Option<Outcome>>),
    Ready(Shared<H>),
}

struct Entry<H: Handler + 'static> {
    id: u64,
    state: State<H>,
    leases: usize,
    /// Bumped when the last lease goes; the linger only closes a login whose
    /// count is the same when it ends.
    idle_gen: u64,
    /// Takes no more tabs (the jump host turned a channel down for lack of
    /// room on this connection).
    full: bool,
    /// Where the password typed for it is kept (the creating tab's
    /// prompted-secret slot), and a copy taken when it was made, in memory
    /// only: a second login for the same key reuses it.
    typed_at: String,
    typed: Option<zeroize::Zeroizing<String>>,
    /// The login of the hop before it, held as long as this one is.
    behind: Option<Lease<H>>,
}

struct Inner<H: Handler + 'static> {
    epoch: u64,
    next_id: u64,
    entries: HashMap<Key, Vec<Entry<H>>>,
}

pub(crate) struct Pool<H: Handler + 'static> {
    inner: Mutex<Inner<H>>,
    me: Weak<Pool<H>>,
}

/// A login to share, or one to make.
pub(crate) enum Acquired<H: Handler + 'static> {
    Shared(Shared<H>, Lease<H>),
    Create(Reservation<H>),
}

impl<H: Handler + 'static> std::fmt::Debug for Acquired<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Acquired::Shared(_, lease) => write!(f, "Shared(login {})", lease.id),
            Acquired::Create(r) => write!(f, "Create(login {})", r.id),
        }
    }
}

/// A tab's share of a login: dropped, the login may close (`LINGER`).
pub struct Lease<H: Handler + 'static> {
    pool: Weak<Pool<H>>,
    key: Key,
    id: u64,
}

impl<H: Handler + 'static> Lease<H> {
    /// The login's id, for the hop reached through it (`Via::Through`).
    pub(crate) fn login_id(&self) -> u64 {
        self.id
    }
}

impl<H: Handler + 'static> Drop for Lease<H> {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            pool.release(&self.key, self.id);
        }
    }
}

/// The tab making a login: `ready` with it, or dropped, which tells the
/// waiting tabs how it ended (`fail`, else `Outcome::Abandoned`).
pub(crate) struct Reservation<H: Handler + 'static> {
    pool: Weak<Pool<H>>,
    key: Key,
    id: u64,
    tx: watch::Sender<Option<Outcome>>,
    outcome: Outcome,
    /// For a second login of the same key: the password typed for the
    /// first one, or where it is kept, to copy into this tab's slot.
    pub(crate) typed: Option<zeroize::Zeroizing<String>>,
    pub(crate) typed_at: Option<String>,
}

impl<H: Handler + 'static> Reservation<H> {
    /// How the login failed, said to the waiting tabs when this drops.
    pub(crate) fn fail(&mut self, outcome: Outcome) {
        self.outcome = outcome;
    }

    /// The login is made: the waiting tabs share it. `behind`: a lease on the
    /// login of the hop before it, held as long as this one is. `typed`: the
    /// password typed for it, if one was, for a second login of the same key.
    /// `None` when the pool no longer wants it (the profile closed
    /// meanwhile): the tab keeps it to itself.
    pub(crate) fn ready(mut self, login: Shared<H>, behind: Option<Lease<H>>, typed: Option<zeroize::Zeroizing<String>>) -> Option<Lease<H>> {
        let pool = self.pool.upgrade()?;
        let stale;
        let lease = {
            let mut inner = pool.lock();
            let entry = inner.entries.get_mut(&self.key).and_then(|list| list.iter_mut().find(|e| e.id == self.id));
            match entry {
                Some(e) => {
                    e.state = State::Ready(login);
                    e.typed = typed;
                    stale = std::mem::replace(&mut e.behind, behind);
                    Some(Lease { pool: Weak::clone(&self.pool), key: self.key.clone(), id: self.id })
                }
                None => {
                    stale = behind;
                    None
                }
            }
        };
        drop(stale);
        self.outcome = if lease.is_some() { Outcome::Ready } else { Outcome::Abandoned };
        lease
    }
}

impl<H: Handler + 'static> Drop for Reservation<H> {
    fn drop(&mut self) {
        if self.outcome != Outcome::Ready {
            if let Some(pool) = self.pool.upgrade() {
                let gone = {
                    let mut inner = pool.lock();
                    take_entry(&mut inner, &self.key, self.id)
                };
                drop(gone);
            }
        }
        self.tx.send_replace(Some(self.outcome));
    }
}

/// Removes login `id` of `key`, to be dropped by the caller once the lock is
/// let go (its `behind` lease locks the pool).
fn take_entry<H: Handler + 'static>(inner: &mut Inner<H>, key: &Key, id: u64) -> Option<Entry<H>> {
    let list = inner.entries.get_mut(key)?;
    let at = list.iter().position(|e| e.id == id)?;
    let entry = list.remove(at);
    if list.is_empty() {
        inner.entries.remove(key);
    }
    Some(entry)
}

impl<H: Handler + 'static> Pool<H> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new_cyclic(|me| Pool { inner: Mutex::new(Inner { epoch: 1, next_id: 1, entries: HashMap::new() }), me: Weak::clone(me) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<H>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A login for `key` to share, or the turn to make it. `typed_at`: where
    /// a password typed for it will be kept, for a second login of the same
    /// key. Waits while another tab makes it (`waiting` is called once then),
    /// and gives up when `attempt` is replaced or its tab closed.
    pub(crate) async fn acquire(
        &self,
        mut key: Key,
        typed_at: &str,
        attempt: &crate::ssh_manager::ConnectAttempt,
        waiting: impl FnOnce(),
    ) -> Result<Acquired<H>, Waited> {
        let mut waiting = Some(waiting);
        loop {
            let (rx, closed) = {
                let mut inner = self.lock();
                key.epoch = inner.epoch;
                let list = inner.entries.entry(key.clone()).or_default();
                // A login that dropped is replaced: its tabs see it end and
                // reconnect, and the first one back makes the new one.
                let mut closed = Vec::new();
                let mut i = 0;
                while i < list.len() {
                    if matches!(&list[i].state, State::Ready(h) if h.is_closed()) {
                        closed.push(list.remove(i));
                    } else {
                        i += 1;
                    }
                }
                let room = |e: &Entry<H>| !e.full && e.leases < CAP;
                if let Some(e) = list.iter_mut().find(|e| room(e) && matches!(e.state, State::Ready(_))) {
                    let State::Ready(h) = &e.state else { unreachable!() };
                    let h = Arc::clone(h);
                    e.leases += 1;
                    e.idle_gen += 1;
                    let lease = Lease { pool: Weak::clone(&self.me), key: key.clone(), id: e.id };
                    drop(inner);
                    drop(closed);
                    return Ok(Acquired::Shared(h, lease));
                }
                // One login of a key is made at a time: a second one only once
                // the first is ready (and full), so its prompts come one after
                // another, and a password typed for the first is there for it.
                let creating = list.iter().find_map(|e| match &e.state {
                    State::Creating(rx) => Some(rx.clone()),
                    State::Ready(_) => None,
                });
                match creating {
                    Some(rx) => (rx, closed),
                    None => {
                        let first = list.iter().find(|e| matches!(e.state, State::Ready(_)));
                        let (typed, first_at) = (first.and_then(|e| e.typed.clone()), first.map(|e| e.typed_at.clone()));
                        let (tx, rx) = watch::channel(None);
                        let id = inner.next_id;
                        inner.next_id += 1;
                        let entry = Entry { id, state: State::Creating(rx), leases: 1, idle_gen: 0, full: false, typed_at: typed_at.to_string(), typed: None, behind: None };
                        inner.entries.entry(key.clone()).or_default().push(entry);
                        drop(inner);
                        drop(closed);
                        return Ok(Acquired::Create(Reservation {
                            pool: Weak::clone(&self.me),
                            key,
                            id,
                            tx,
                            outcome: Outcome::Abandoned,
                            typed,
                            typed_at: first_at,
                        }));
                    }
                }
            };
            drop(closed);
            if let Some(w) = waiting.take() {
                w();
            }
            let outcome = tokio::select! {
                o = wait_for(rx) => o,
                _ = attempt.superseded() => return Err(Waited::Cancelled),
            };
            match outcome {
                Outcome::Ready | Outcome::Abandoned => continue,
                Outcome::Failed { auth, retry } => return Err(Waited::Failed { auth, retry }),
            }
        }
    }

    /// One more lease on a login, for the hop chained behind it.
    pub(crate) fn lease_again(&self, lease: &Lease<H>) -> Option<Lease<H>> {
        let mut inner = self.lock();
        let e = inner.entries.get_mut(&lease.key)?.iter_mut().find(|e| e.id == lease.id)?;
        e.leases += 1;
        e.idle_gen += 1;
        Some(Lease { pool: Weak::clone(&self.me), key: lease.key.clone(), id: lease.id })
    }

    fn release(&self, key: &Key, id: u64) {
        let idle = {
            let mut inner = self.lock();
            let Some(e) = inner.entries.get_mut(key).and_then(|l| l.iter_mut().find(|e| e.id == id)) else { return };
            e.leases = e.leases.saturating_sub(1);
            if e.leases > 0 {
                return;
            }
            e.idle_gen += 1;
            e.idle_gen
        };
        let Some(me) = self.me.upgrade() else { return };
        let key = key.clone();
        let close_if_idle = move |me: &Pool<H>| {
            let gone = {
                let mut inner = me.lock();
                let still_idle = inner
                    .entries
                    .get(&key)
                    .and_then(|l| l.iter().find(|e| e.id == id))
                    .is_some_and(|e| e.leases == 0 && e.idle_gen == idle);
                if still_idle { take_entry(&mut inner, &key, id) } else { None }
            };
            drop(gone);
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(async move {
                    tokio::time::sleep(LINGER).await;
                    close_if_idle(&me);
                });
            }
            // No runtime to wait in (the app is closing): close it now.
            Err(_) => close_if_idle(&me),
        }
    }

    /// Forget every login: the profile closed. A login still being made
    /// can't come back into the next profile's pool (`epoch`).
    pub(crate) fn clear(&self) {
        let gone = {
            let mut inner = self.lock();
            inner.epoch += 1;
            std::mem::take(&mut inner.entries)
        };
        drop(gone);
    }

    /// The logins made for `login` (`jump<id>`) take no more tabs: that jump
    /// host was deleted. Tabs on them carry on.
    pub(crate) fn forget_login(&self, login: &str) {
        let gone: Vec<Vec<Entry<H>>> = {
            let mut inner = self.lock();
            let keys: Vec<Key> = inner.entries.keys().filter(|k| k.login == login).cloned().collect();
            keys.iter().filter_map(|k| inner.entries.remove(k)).collect()
        };
        drop(gone);
    }

    /// Logins no tab uses any more close now (sharing was turned off).
    pub(crate) fn drop_idle(&self) {
        let gone: Vec<Entry<H>> = {
            let mut inner = self.lock();
            let mut gone = Vec::new();
            for list in inner.entries.values_mut() {
                let mut i = 0;
                while i < list.len() {
                    if list[i].leases == 0 && matches!(list[i].state, State::Ready(_)) {
                        gone.push(list.remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            inner.entries.retain(|_, l| !l.is_empty());
            gone
        };
        drop(gone);
    }

    /// A channel couldn't be opened on the login at `addr` (`Handle`'s
    /// address): `full`, the jump host has no room for more on it, and it
    /// takes no more tabs; otherwise it isn't answering and goes, so the next
    /// tab logs in afresh.
    pub(crate) fn channel_failed(&self, addr: usize, full: bool) {
        let gone = {
            let mut inner = self.lock();
            let mut gone = None;
            'find: for (key, list) in inner.entries.iter_mut() {
                for e in list.iter_mut() {
                    if matches!(&e.state, State::Ready(h) if Arc::as_ptr(h) as usize == addr) {
                        if full {
                            e.full = true;
                        } else {
                            gone = Some((key.clone(), e.id));
                        }
                        break 'find;
                    }
                }
            }
            gone.and_then(|(key, id)| take_entry(&mut inner, &key, id))
        };
        drop(gone);
    }
}

/// How a login being made ended. A sender gone without a word: abandoned.
async fn wait_for(mut rx: watch::Receiver<Option<Outcome>>) -> Outcome {
    loop {
        if let Some(o) = *rx.borrow_and_update() {
            return o;
        }
        if rx.changed().await.is_err() {
            return rx.borrow().unwrap_or(Outcome::Abandoned);
        }
    }
}

// ── The setting ───────────────────────────────────────────────────────────

const SETTING_FILE: &str = "jump_share.json";
/// Until Settings says otherwise.
pub(crate) const DEFAULT_ON: bool = false;

/// 0 = not read yet, 1 = off, 2 = on.
static SHARING: AtomicU8 = AtomicU8::new(0);

#[derive(serde::Serialize, serde::Deserialize)]
struct SettingFile {
    on: bool,
}

/// Whether SSH terminal tabs share jump host logins.
pub(crate) fn sharing_on(app: &tauri::AppHandle) -> bool {
    match SHARING.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = crate::local_open::read_setting::<SettingFile>(app, SETTING_FILE).map(|f| f.on).unwrap_or(DEFAULT_ON);
            SHARING.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

#[tauri::command]
pub async fn get_jump_sharing(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(sharing_on(&app))
}

/// Turned off, logins no tab uses close now; tabs already on a shared login
/// keep it until they close or reconnect.
#[tauri::command]
pub async fn set_jump_sharing(app: tauri::AppHandle, on: bool) -> Result<bool, String> {
    crate::local_open::write_setting(&app, SETTING_FILE, &SettingFile { on }, "the jump host sharing setting")?;
    SHARING.store(if on { 2 } else { 1 }, Ordering::Relaxed);
    if !on {
        POOL.drop_idle();
    }
    Ok(on)
}

/// The profile closed (`SshState::forget_fork_sessions`).
pub(crate) fn clear() {
    POOL.clear();
}

/// Jump host `jump_id` was deleted (jump_gone.rs): its logins take no more
/// tabs.
pub(crate) fn forget_saved(jump_id: i64) {
    POOL.forget_login(&format!("jump{jump_id}"));
}

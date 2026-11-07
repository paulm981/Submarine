//! What it takes to open many servers behind one jump host at once (a whole
//! folder, say): a login queue per jump host. At most `limit` sessions log
//! in to the same jump host at a time (default 8, 0 = no limit), the rest
//! wait their turn. A bastion counts logins that haven't finished as
//! "startups" and drops connections past its MaxStartups (OpenSSH's default
//! starts refusing at 10), so opening a folder of 100 servers behind one used
//! to fail most of them. Servers reached directly aren't queued.
//!
//! Also the `walls` table's schema. The fork's own Walls are gone (the
//! original project's Wall and Broadcast are back), but a profile keeps the
//! walls it saved: the table is still made, and nothing reads or writes it.

use crate::ssh_manager::ConnectAttempt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// ── Login queue per jump host ──────────────────────────────────────────────

const LOGIN_LIMIT_FILE: &str = "jump_login_limit.json";
pub(crate) const DEFAULT_LOGIN_LIMIT: i64 = 8;
const MAX_LOGIN_LIMIT: i64 = 256;

/// -1 until read from the settings file.
static LOGIN_LIMIT: AtomicI64 = AtomicI64::new(-1);

/// Jump host → (the limit it was made with, its slots). A changed limit
/// gets new slots; logins holding the old ones finish as they were. A jump
/// host is known by its address (`hop_policy::queue_key`), so a saved jump
/// host and the same bastion in a ~/.ssh/config ProxyJump share one queue.
type SlotMap = HashMap<String, (i64, Arc<Semaphore>)>;
static LOGIN_SLOTS: Mutex<Option<SlotMap>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct LimitFile {
    limit: i64,
}

fn login_limit(app: &tauri::AppHandle) -> i64 {
    let cached = LOGIN_LIMIT.load(Ordering::Relaxed);
    if cached >= 0 {
        return cached;
    }
    let limit = crate::local_open::read_setting::<LimitFile>(app, LOGIN_LIMIT_FILE)
        .map(|f| f.limit.clamp(0, MAX_LOGIN_LIMIT))
        .unwrap_or(DEFAULT_LOGIN_LIMIT);
    LOGIN_LIMIT.store(limit, Ordering::Relaxed);
    limit
}

fn slots_for(key: &str, limit: i64) -> Arc<Semaphore> {
    let mut guard = LOGIN_SLOTS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(key) {
        Some((made_with, sem)) if *made_with == limit => Arc::clone(sem),
        _ => {
            let sem = Arc::new(Semaphore::new(limit as usize));
            map.insert(key.to_string(), (limit, Arc::clone(&sem)));
            sem
        }
    }
}

/// A turn to log in to the jump host `key` (`hop_policy::queue_key`), held
/// until dropped: right away while
/// fewer than the limit are logging in, otherwise once one of them is done —
/// first come, first served. `None` with no limit set. `waiting` is called
/// once if the turn doesn't come straight away (for the session's log).
/// Gives up when the attempt is replaced or its tab closed. A limit changed
/// in Settings applies to logins already waiting too: they queue again under
/// it (0 lets them all go).
pub(crate) async fn login_turn(
    app: &tauri::AppHandle,
    key: &str,
    attempt: &ConnectAttempt,
    waiting: impl FnOnce(i64),
) -> Result<Option<OwnedSemaphorePermit>, String> {
    queue_for_login(key, Some(attempt), waiting, || login_limit(app)).await
}

/// A turn to log in to this jump host, for a connection with no connect
/// attempt (a monitor's): as `login_turn`, but it waits until it gets one
/// or is dropped (the monitor's stop signal drops it).
pub(crate) async fn login_turn_until_dropped(app: &tauri::AppHandle, key: &str) -> Option<OwnedSemaphorePermit> {
    // Without an attempt nothing gives up but a closed queue, and the queues
    // are never closed.
    queue_for_login(key, None, |_| {}, || login_limit(app)).await.ok().flatten()
}

/// `limit`: the setting, read again while waiting.
async fn queue_for_login(
    key: &str,
    attempt: Option<&ConnectAttempt>,
    waiting: impl FnOnce(i64),
    limit: impl Fn() -> i64,
) -> Result<Option<OwnedSemaphorePermit>, String> {
    let mut waiting = Some(waiting);
    let login_limit = limit;
    loop {
        let limit = login_limit();
        if limit == 0 {
            return Ok(None);
        }
        let on_wait = || {
            if let Some(w) = waiting.take() {
                w(limit)
            }
        };
        let turn = wait_for_turn(slots_for(key, limit), attempt, on_wait, || login_limit() == limit).await?;
        if let Turn::Got(permit) = turn {
            return Ok(Some(permit));
        }
    }
}

enum Turn {
    Got(OwnedSemaphorePermit),
    /// The limit changed while waiting: queue again under the new one.
    LimitChanged,
}

/// `attempt`: gives up once it's replaced or its tab closed. `None` waits on.
async fn wait_for_turn(
    sem: Arc<Semaphore>,
    attempt: Option<&ConnectAttempt>,
    on_wait: impl FnOnce(),
    same_limit: impl Fn() -> bool,
) -> Result<Turn, String> {
    if let Ok(permit) = Arc::clone(&sem).try_acquire_owned() {
        return Ok(Turn::Got(permit));
    }
    on_wait();
    // One acquire, polled until it completes, keeps this login's place in the
    // queue; the timer only checks whether it is still wanted.
    let acquire = sem.acquire_owned();
    tokio::pin!(acquire);
    loop {
        tokio::select! {
            permit = &mut acquire => {
                return permit.map(Turn::Got).map_err(|_| "jump host login queue closed".to_string());
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                if let Some(attempt) = attempt {
                    if !attempt.is_current().await {
                        return Err("stopped waiting for a jump host login slot".into());
                    }
                }
                if !same_limit() {
                    return Ok(Turn::LimitChanged);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn get_jump_login_limit(app: tauri::AppHandle) -> Result<i64, String> {
    Ok(login_limit(&app))
}

#[tauri::command]
pub async fn set_jump_login_limit(app: tauri::AppHandle, limit: i64) -> Result<i64, String> {
    let limit = limit.clamp(0, MAX_LOGIN_LIMIT);
    crate::local_open::write_setting(&app, LOGIN_LIMIT_FILE, &LimitFile { limit }, "the jump host login limit")?;
    LOGIN_LIMIT.store(limit, Ordering::Relaxed);
    Ok(limit)
}

// ── Saved walls (no longer used) ───────────────────────────────────────────

/// Kept so a profile's saved walls aren't lost: made as before, never read.
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS walls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    server_ids TEXT NOT NULL DEFAULT '[]',
    cols INTEGER NOT NULL DEFAULT 0
)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_limit() {
        let a = slots_for("test-slots:7", 2);
        assert!(Arc::ptr_eq(&a, &slots_for("test-slots:7", 2)));
        let p1 = Arc::clone(&a).try_acquire_owned().unwrap();
        let _p2 = Arc::clone(&a).try_acquire_owned().unwrap();
        assert!(Arc::clone(&a).try_acquire_owned().is_err());
        drop(p1);
        assert!(Arc::clone(&a).try_acquire_owned().is_ok());
        // A new limit, new slots; another jump host has its own.
        assert!(!Arc::ptr_eq(&a, &slots_for("test-slots:7", 3)));
        assert!(!Arc::ptr_eq(&slots_for("test-slots:7", 3), &slots_for("test-slots:8", 3)));
    }

    #[tokio::test]
    async fn a_login_waits_for_a_turn_and_gives_up_when_its_tab_moves_on() {
        let generations = Arc::new(tokio::sync::Mutex::new(HashMap::from([("s".to_string(), 1u64)])));
        let attempt = ConnectAttempt::new(Arc::clone(&generations), "s".into(), 1);
        let sem = Arc::new(Semaphore::new(1));
        let held = Arc::clone(&sem).try_acquire_owned().unwrap();

        // Waits (and says so) until the login ahead of it is done.
        let mut said = false;
        let turn = {
            let (sem, attempt) = (Arc::clone(&sem), attempt.clone());
            tokio::spawn(async move {
                let r = wait_for_turn(sem, Some(&attempt), || said = true, || true).await;
                (matches!(r, Ok(Turn::Got(_))), said)
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!turn.is_finished());
        drop(held);
        assert_eq!(turn.await.unwrap(), (true, true));

        // A new limit while waiting: it queues again under that one.
        let _held = Arc::clone(&sem).try_acquire_owned().unwrap();
        assert!(matches!(wait_for_turn(Arc::clone(&sem), Some(&attempt), || {}, || false).await, Ok(Turn::LimitChanged)));

        // The tab reconnected or closed while waiting: it stops waiting.
        generations.lock().await.insert("s".into(), 2);
        assert!(wait_for_turn(Arc::clone(&sem), Some(&attempt), || {}, || true).await.is_err());
    }

    #[tokio::test]
    async fn a_monitors_login_waits_for_a_turn_until_its_stopped() {
        let sem = Arc::new(Semaphore::new(1));
        let held = Arc::clone(&sem).try_acquire_owned().unwrap();

        // No attempt to give up with: it waits past the checks for one...
        let turn = tokio::spawn(wait_for_turn(Arc::clone(&sem), None, || {}, || true));
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        assert!(!turn.is_finished());
        // ...gets the turn when the login ahead of it is done...
        drop(held);
        assert!(matches!(turn.await.unwrap(), Ok(Turn::Got(_))));

        // ...and a stopped monitor (its wait dropped) leaves the queue: the
        // login after it isn't held up.
        let held = Arc::clone(&sem).try_acquire_owned().unwrap();
        let stopped = tokio::spawn(wait_for_turn(Arc::clone(&sem), None, || {}, || true));
        let next = tokio::spawn(wait_for_turn(Arc::clone(&sem), None, || {}, || true));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        stopped.abort();
        drop(held);
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), next).await.expect("the next login's turn");
        assert!(matches!(got.unwrap(), Ok(Turn::Got(_))));
    }

    /// Many tabs opening at once behind one bastion: no more than the limit
    /// log in to it together, every one gets its turn, and another bastion
    /// isn't held up by them (#43, D1).
    #[tokio::test]
    async fn many_opens_behind_one_bastion_keep_to_the_limit() {
        use std::sync::atomic::AtomicUsize;
        let key = crate::hop_policy::queue_key("Bastion.Example", 22);
        let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let mut logins = Vec::new();
        for _ in 0..20 {
            let (key, now, most) = (key.clone(), Arc::clone(&now), Arc::clone(&most));
            logins.push(tokio::spawn(async move {
                let _turn = queue_for_login(&key, None, |_| {}, || 3).await.unwrap().expect("a turn");
                let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(n, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                now.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        // Another bastion gets its turn at once meanwhile.
        let other = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            queue_for_login(&crate::hop_policy::queue_key("other.example", 22), None, |_| {}, || 3),
        )
        .await
        .expect("not held up by another bastion's queue");
        assert!(other.unwrap().is_some());
        for l in logins {
            tokio::time::timeout(std::time::Duration::from_secs(10), l).await.expect("every login gets a turn").unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 3);
        // No limit: no queue.
        assert!(queue_for_login(&key, None, |_| {}, || 0).await.unwrap().is_none());
    }

    /// Two routes through the same two bastions in opposite orders, with one
    /// login at a time on each: as each hop lets go of its turn once it's
    /// logged in, before the next hop queues, neither waits on the other.
    #[tokio::test]
    async fn routes_through_the_same_bastions_in_opposite_orders_both_get_through() {
        let a = crate::hop_policy::queue_key("a.example", 22);
        let b = crate::hop_policy::queue_key("b.example", 22);
        let route = |hops: Vec<String>| async move {
            for hop in hops {
                let turn = queue_for_login(&hop, None, |_| {}, || 1).await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(30)).await; // the login
                drop(turn);
            }
        };
        let both = async { tokio::join!(route(vec![a.clone(), b.clone()]), route(vec![b, a])) };
        tokio::time::timeout(std::time::Duration::from_secs(5), both).await.expect("no deadlock between the routes");
    }
}

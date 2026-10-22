//! What closing a session would cut short, for the page to ask about first:
//! file transfers running on it (uploads, downloads, a long delete), archive
//! and extract jobs running on the server, mirrors running, and Live Edit
//! copies with edits that never reached the server. Closing a Wall that took
//! tabs asks about these even when it's set to close them without asking.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

use crate::mirror::MirrorMap;

/// What a running job is, for the page's wording.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Job {
    Transfer,
    Archive,
}

/// Jobs running, by kind and session id (the plain one: a dedicated `::sftp`
/// connection is only its transport, see get_sftp_session).
fn running() -> &'static StdMutex<HashMap<(Job, String), usize>> {
    static RUNNING: std::sync::OnceLock<StdMutex<HashMap<(Job, String), usize>>> = std::sync::OnceLock::new();
    RUNNING.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Counts one job on a session for as long as it's held.
pub(crate) struct RunningJob(Job, String);

impl RunningJob {
    fn start(job: Job, session_id: &str) -> Self {
        *running().lock().unwrap_or_else(|e| e.into_inner()).entry((job, session_id.to_string())).or_insert(0) += 1;
        RunningJob(job, session_id.to_string())
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        let mut map = running().lock().unwrap_or_else(|e| e.into_inner());
        let key = (self.0, std::mem::take(&mut self.1));
        if let Some(n) = map.get_mut(&key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                map.remove(&key);
            }
        }
    }
}

/// An upload, download or long delete, counted while it runs.
pub(crate) struct RunningTransfer;

impl RunningTransfer {
    pub(crate) fn start(session_id: &str) -> RunningJob {
        RunningJob::start(Job::Transfer, session_id)
    }
}

/// An archive or extract job on the server, counted while it runs.
pub(crate) fn running_archive(session_id: &str) -> RunningJob {
    RunningJob::start(Job::Archive, session_id)
}

fn running_on(job: Job, session_id: &str) -> usize {
    running().lock().unwrap_or_else(|e| e.into_inner()).get(&(job, session_id.to_string())).copied().unwrap_or(0)
}

/// Live Edit copies of this session's with edits not uploaded yet.
fn unsent_edits_on(session_id: &str) -> usize {
    let Ok(dir) = crate::session_sftp_dir(session_id) else { return 0 };
    let copies = crate::live_edit_copies().lock().unwrap_or_else(|e| e.into_inner());
    copies.iter().filter(|(path, synced)| crate::is_unsent_under(path, synced, &dir)).count()
}

/// Mirrors running on each session: every one that hasn't stopped or failed.
/// Closing the session stops them (mirror::stop_all_for_session).
async fn mirrors_running(mirrors: &MirrorMap) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for m in crate::mirror::list(mirrors, None).await {
        if m.state != "stopped" && m.state != "error" {
            *out.entry(m.session_id).or_insert(0) += 1;
        }
    }
    out
}

#[derive(serde::Serialize)]
pub(crate) struct SessionBusy {
    session_id: String,
    transfers: usize,
    archives: usize,
    mirrors: usize,
    unsent_edits: usize,
}

async fn busy_of(mirrors: &MirrorMap, session_ids: Vec<String>) -> Vec<SessionBusy> {
    let mirrors = mirrors_running(mirrors).await;
    session_ids
        .into_iter()
        .filter_map(|session_id| {
            let busy = SessionBusy {
                transfers: running_on(Job::Transfer, &session_id),
                archives: running_on(Job::Archive, &session_id),
                mirrors: mirrors.get(&session_id).copied().unwrap_or(0),
                unsent_edits: unsent_edits_on(&session_id),
                session_id,
            };
            (busy.transfers + busy.archives + busy.mirrors + busy.unsent_edits > 0).then_some(busy)
        })
        .collect()
}

/// The sessions of `session_ids` that are busy, with what they're busy with.
/// Ones with nothing running are left out.
#[tauri::command]
pub(crate) async fn sessions_busy(
    mirrors: tauri::State<'_, MirrorMap>,
    session_ids: Vec<String>,
) -> Result<Vec<SessionBusy>, String> {
    Ok(busy_of(mirrors.inner(), session_ids).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transfer_counts_while_it_runs() {
        let id = "session-busy-test-1";
        assert_eq!(running_on(Job::Transfer, id), 0);
        let a = RunningTransfer::start(id);
        let b = RunningTransfer::start(id);
        assert_eq!(running_on(Job::Transfer, id), 2);
        assert_eq!(running_on(Job::Archive, id), 0, "a transfer isn't an archive job");
        drop(a);
        assert_eq!(running_on(Job::Transfer, id), 1);
        drop(b);
        assert_eq!(running_on(Job::Transfer, id), 0);
        assert!(!running().lock().unwrap().contains_key(&(Job::Transfer, id.to_string())));
    }

    #[tokio::test]
    async fn an_archive_job_counts_while_it_runs() {
        let id = "session-busy-test-4";
        let job = running_archive(id);
        let busy = busy_of(&MirrorMap::default(), vec![id.to_string()]).await;
        assert_eq!(busy.len(), 1);
        assert_eq!((busy[0].archives, busy[0].transfers), (1, 0));
        drop(job);
        assert!(busy_of(&MirrorMap::default(), vec![id.to_string()]).await.is_empty());
    }

    fn mirror(session_id: &str, state: &str) -> crate::mirror::ActiveMirror {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        crate::mirror::ActiveMirror {
            status: Arc::new(Mutex::new(crate::mirror::MirrorStatus {
                id: format!("mir-{session_id}-{state}"),
                session_id: session_id.to_string(),
                local: String::new(),
                remote: String::new(),
                state: state.to_string(),
                queue_depth: 0,
                uploaded: 0,
                downloaded: 0,
                deleted: 0,
                scanned: 0,
                transfer_total: 0,
                transfer_done: 0,
                last_event_ms: 0,
                error: None,
            })),
            stop_tx: Mutex::new(None),
            join: Mutex::new(None),
        }
    }

    #[tokio::test]
    async fn running_mirrors_count_and_stopped_ones_dont() {
        let map = MirrorMap::default();
        {
            let mut m = map.lock().await;
            m.insert("a".into(), mirror("session-busy-test-5", "watching"));
            m.insert("b".into(), mirror("session-busy-test-5", "initial-sync"));
            m.insert("c".into(), mirror("session-busy-test-5", "stopped"));
            m.insert("d".into(), mirror("session-busy-test-6", "error"));
        }
        let busy = busy_of(&map, vec!["session-busy-test-5".into(), "session-busy-test-6".into()]).await;
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].session_id, "session-busy-test-5");
        assert_eq!(busy[0].mirrors, 2);
    }

    #[tokio::test]
    async fn unsent_live_edits_count_and_sent_ones_dont() {
        let id = "session-busy-test-2";
        let dir = crate::session_sftp_dir(id).unwrap().join("f");
        std::fs::create_dir_all(&dir).unwrap();
        let sent = dir.join("sent.txt");
        let edited = dir.join("edited.txt");
        std::fs::write(&sent, b"same").unwrap();
        std::fs::write(&edited, b"changed").unwrap();
        {
            let mut copies = crate::live_edit_copies().lock().unwrap();
            copies.insert(sent.clone(), crate::sha256_of(b"same"));
            copies.insert(edited.clone(), crate::sha256_of(b"original"));
        }
        let busy = busy_of(&MirrorMap::default(), vec![id.to_string(), "session-busy-test-3".to_string()]).await;
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].session_id, id);
        assert_eq!(busy[0].unsent_edits, 1);
        assert_eq!(busy[0].transfers, 0);
        {
            let mut copies = crate::live_edit_copies().lock().unwrap();
            copies.remove(&sent);
            copies.remove(&edited);
        }
        let _ = std::fs::remove_dir_all(crate::session_sftp_dir(id).unwrap());
    }
}

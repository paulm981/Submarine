//! What closing a session would cut short, for the page to ask about first:
//! file transfers running on it (uploads, downloads, a long delete) and Live
//! Edit copies with edits that never reached the server. Closing a Wall that
//! took tabs asks about these even when it's set to close them without
//! asking.

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

/// Transfers running, by session id (the plain one: a dedicated `::sftp`
/// connection is only its transport, see get_sftp_session).
fn running() -> &'static StdMutex<HashMap<String, usize>> {
    static RUNNING: std::sync::OnceLock<StdMutex<HashMap<String, usize>>> = std::sync::OnceLock::new();
    RUNNING.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Counts one transfer on a session for as long as it's held.
pub(crate) struct RunningTransfer(String);

impl RunningTransfer {
    pub(crate) fn start(session_id: &str) -> Self {
        *running().lock().unwrap_or_else(|e| e.into_inner()).entry(session_id.to_string()).or_insert(0) += 1;
        RunningTransfer(session_id.to_string())
    }
}

impl Drop for RunningTransfer {
    fn drop(&mut self) {
        let mut map = running().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = map.get_mut(&self.0) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                map.remove(&self.0);
            }
        }
    }
}

fn transfers_on(session_id: &str) -> usize {
    running().lock().unwrap_or_else(|e| e.into_inner()).get(session_id).copied().unwrap_or(0)
}

/// Live Edit copies of this session's with edits not uploaded yet.
fn unsent_edits_on(session_id: &str) -> usize {
    let Ok(dir) = crate::session_sftp_dir(session_id) else { return 0 };
    let copies = crate::live_edit_copies().lock().unwrap_or_else(|e| e.into_inner());
    copies.iter().filter(|(path, synced)| crate::is_unsent_under(path, synced, &dir)).count()
}

#[derive(serde::Serialize)]
pub(crate) struct SessionBusy {
    session_id: String,
    transfers: usize,
    unsent_edits: usize,
}

/// The sessions of `session_ids` that are busy, with what they're busy with.
/// Ones with nothing running are left out.
#[tauri::command]
pub(crate) fn sessions_busy(session_ids: Vec<String>) -> Vec<SessionBusy> {
    session_ids
        .into_iter()
        .filter_map(|session_id| {
            let transfers = transfers_on(&session_id);
            let unsent_edits = unsent_edits_on(&session_id);
            (transfers > 0 || unsent_edits > 0).then_some(SessionBusy { session_id, transfers, unsent_edits })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transfer_counts_while_it_runs() {
        let id = "session-busy-test-1";
        assert_eq!(transfers_on(id), 0);
        let a = RunningTransfer::start(id);
        let b = RunningTransfer::start(id);
        assert_eq!(transfers_on(id), 2);
        drop(a);
        assert_eq!(transfers_on(id), 1);
        drop(b);
        assert_eq!(transfers_on(id), 0);
        assert!(!running().lock().unwrap().contains_key(id));
    }

    #[test]
    fn unsent_live_edits_count_and_sent_ones_dont() {
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
        let busy = sessions_busy(vec![id.to_string(), "session-busy-test-3".to_string()]);
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

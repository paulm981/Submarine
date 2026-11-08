//! One upload at a time per remote file (`RemoteWriteLocks`): a second
//! upload to the same file waits for the first.
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use remote_writes::*`.

use crate::*;

/// Remote files an upload is writing right now, one lock per (session, path).
/// Two uploads to the same remote file would interleave their writes and leave
/// a mix of both, so the second one waits here until the first has closed the
/// file (it then overwrites it whole, as it would have a moment later anyway).
#[derive(Default)]
pub(crate) struct RemoteWriteLocks {
    pub(crate) map: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl RemoteWriteLocks {
    /// The lock for this file. Entries nobody holds or waits on any more are
    /// dropped here, so the map doesn't grow with every file ever uploaded.
    pub(crate) fn lock_for(&self, session_id: &str, remote_path: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, l| Arc::strong_count(l) > 1);
        Arc::clone(map.entry(format!("{session_id}\0{remote_path}")).or_default())
    }

    /// Wait for the file's turn. Gives up with `None` if the user cancels
    /// before or while it is queued, so a stuck first upload can't trap the
    /// second. The flag says whether it had to wait: another upload was
    /// writing the file meanwhile, so it may be there now.
    pub(crate) async fn acquire(
        &self,
        session_id: &str,
        remote_path: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<(tokio::sync::OwnedMutexGuard<()>, bool)> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { return None; }
        let lock = self.lock_for(session_id, remote_path);
        if let Ok(guard) = Arc::clone(&lock).try_lock_owned() {
            return Some((guard, false));
        }
        let acquire = lock.lock_owned();
        tokio::pin!(acquire);
        loop {
            tokio::select! {
                guard = &mut acquire => return Some((guard, true)),
                _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) { return None; }
                }
            }
        }
    }
}

pub(crate) static REMOTE_WRITE_LOCKS: std::sync::OnceLock<RemoteWriteLocks> = std::sync::OnceLock::new();

pub(crate) fn remote_write_locks() -> &'static RemoteWriteLocks {
    REMOTE_WRITE_LOCKS.get_or_init(RemoteWriteLocks::default)
}

/// sftp_upload_file's turn at the remote file (lib.rs). Another upload to
/// this same remote file would interleave with ours, so it waits for it to
/// finish first (the card shows 0% meanwhile). What the file looks like is
/// asked again: the upload ahead may have just created it, which this one
/// was told isn't there; then it asks first, as the check at the top would
/// have. The turn, held for the upload, and whether the file is there.
pub(crate) async fn upload_turn(
    sftp: &russh_sftp::client::SftpSession,
    session_id: &str,
    remote_path: &str,
    cancel: &std::sync::atomic::AtomicBool,
    overwrite: Option<bool>,
    name: &str,
    emit_progress: &(dyn Fn(u64, &str, Option<String>) + Sync),
) -> Result<(tokio::sync::OwnedMutexGuard<()>, bool), String> {
    let Some((write_turn, _)) = remote_write_locks().acquire(session_id, remote_path, cancel).await else {
        emit_progress(0, "cancelled", None);
        return Err("cancelled".into());
    };
    let existed = sftp.metadata(remote_path).await.is_ok();
    if existed && overwrite != Some(true) {
        emit_progress(0, "error", Some(format!("{name} is already on the server.")));
        return Err(format!("EXISTS:{}", remote_path));
    }
    Ok((write_turn, existed))
}

/// upload_dir_file's turn at the remote file (lib.rs): it waits while
/// another upload writes it, and then `ask_first`, the folder's `EXISTS:`
/// answer, is given if that other upload left a file there.
pub(crate) async fn dir_file_turn(
    sftp: &russh_sftp::client::SftpSession,
    session_id: &str,
    remote_full: &str,
    cancel: &std::sync::atomic::AtomicBool,
    ask_first: Option<String>,
) -> Result<tokio::sync::OwnedMutexGuard<()>, DirFileEnd> {
    let Some((write_turn, waited)) = remote_write_locks().acquire(session_id, remote_full, cancel).await else {
        return Err(DirFileEnd::Stopped(None));
    };
    if let Some(exists) = ask_first.filter(|_| waited) {
        if sftp.metadata(remote_full).await.is_ok() {
            return Err(DirFileEnd::Failed(exists));
        }
    }
    Ok(write_turn)
}

#[cfg(test)]
mod remote_write_lock_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[tokio::test]
    async fn second_upload_to_the_same_file_waits_for_the_first() {
        let locks = RemoteWriteLocks::default();
        let no_cancel = AtomicBool::new(false);
        let first = locks.acquire("s1", "/srv/a", &no_cancel).await.unwrap();
        // Same file: has to wait. A different file or session goes straight in.
        let waited = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            locks.acquire("s1", "/srv/a", &no_cancel),
        )
        .await;
        assert!(waited.is_err());
        assert!(locks.acquire("s1", "/srv/b", &no_cancel).await.is_some());
        assert!(locks.acquire("s2", "/srv/a", &no_cancel).await.is_some());
        drop(first);
        assert!(locks.acquire("s1", "/srv/a", &no_cancel).await.is_some());
    }

    #[tokio::test]
    async fn a_cancel_frees_a_queued_upload() {
        let locks = RemoteWriteLocks::default();
        let no_cancel = AtomicBool::new(false);
        let _first = locks.acquire("s1", "/srv/a", &no_cancel).await.unwrap();
        let cancel = AtomicBool::new(true);
        assert!(locks.acquire("s1", "/srv/a", &cancel).await.is_none());
    }

    #[tokio::test]
    async fn a_cancel_before_queueing_takes_no_turn() {
        let locks = RemoteWriteLocks::default();
        let cancel = AtomicBool::new(true);
        assert!(locks.acquire("s1", "/srv/a", &cancel).await.is_none());
    }

    #[tokio::test]
    async fn says_whether_it_had_to_wait() {
        let locks = Arc::new(RemoteWriteLocks::default());
        let no_cancel = Arc::new(AtomicBool::new(false));
        let (first, waited) = locks.acquire("s1", "/srv/a", &no_cancel).await.unwrap();
        assert!(!waited);
        let (l, c) = (Arc::clone(&locks), Arc::clone(&no_cancel));
        let second = tokio::spawn(async move { l.acquire("s1", "/srv/a", &c).await.map(|(_, w)| w) });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(first);
        assert_eq!(second.await.unwrap(), Some(true));
    }

    #[tokio::test]
    async fn finished_files_are_forgotten() {
        let locks = RemoteWriteLocks::default();
        let no_cancel = AtomicBool::new(false);
        drop(locks.acquire("s1", "/srv/a", &no_cancel).await);
        drop(locks.acquire("s1", "/srv/b", &no_cancel).await);
        assert_eq!(locks.map.lock().unwrap().len(), 1);
    }
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod remote_writes;", 1),
                ("", "use remote_writes::*;", 1),
                ("upload_dir_file", "dir_file_turn(&sftp, &session_id, &remote_full, &cancel, ask_first)", 1),
                ("sftp_upload_file", "upload_turn(&sftp, &session_id, &remote_path, &cancel, overwrite, &name, &emit_progress)", 1),
                ("sftp_open_remote_file", "remote_write_locks(", 1),
            ],
        );
    }
}

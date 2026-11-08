//! Live Edit: a remote file opened in a local editor, its saves uploaded,
//! and edits that never reached the server kept ("unsent edits").
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use live_edit::*`.

use crate::*;

/// Live Edit copies being watched, each with the hash of what the server
/// holds as far as this app knows (what was downloaded, or last uploaded).
/// A copy whose content differs has edits that were never uploaded.
pub(crate) fn live_edit_copies() -> &'static StdMutex<std::collections::HashMap<PathBuf, Vec<u8>>> {
    static COPIES: std::sync::OnceLock<StdMutex<std::collections::HashMap<PathBuf, Vec<u8>>>> = std::sync::OnceLock::new();
    COPIES.get_or_init(|| StdMutex::new(std::collections::HashMap::new()))
}

/// Set by the first Live Edit, to tell the page about kept copies from
/// wherever they're found (a disconnect, a lock).
pub(crate) static LIVE_EDIT_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Where copies with unsent edits are kept: "Submarine unsent edits" in the
/// Downloads folder, where the user will look for a file, or in the app's
/// data folder when there's no Downloads folder.
#[cfg(not(test))]
pub(crate) fn unsent_edits_dir() -> Option<PathBuf> {
    const NAME: &str = "Submarine unsent edits";
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.join(NAME)))
        .or_else(|| LIVE_EDIT_APP.get().and_then(|a| a.path().app_data_dir().ok()).map(|p| p.join(NAME)))
}

/// Tests keep copies in a folder of their own, not in the Downloads folder.
#[cfg(test)]
pub(crate) fn unsent_edits_dir() -> Option<PathBuf> {
    Some(std::env::temp_dir().join(format!("submarine-test-unsent-edits-{}", std::process::id())))
}

/// The names a kept copy may take in its folder: `name`, then `name (2)`,
/// `name (3)`, … before the extension.
pub(crate) fn kept_names(name: &str) -> impl Iterator<Item = String> + '_ {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    std::iter::once(name.to_string()).chain((2..10_000).map(move |n| format!("{stem} ({n}){ext}")))
}

/// Copies the file `src` to `dest`, which must not exist yet: anything
/// there, a link too, makes it fail with `AlreadyExists`, and nothing is
/// written through it. A copy that fails part way is removed. `private`: on
/// Unix the copy is readable by its owner only (0600) from the start.
pub(crate) fn copy_to_new(src: &Path, dest: &Path, private: bool) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private {
            opts.mode(0o600);
        }
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut out = opts.open(dest)?;
    let copied = std::fs::File::open(src)
        .and_then(|mut from| std::io::copy(&mut from, &mut out))
        .and_then(|_| out.sync_all());
    drop(out);
    if let Err(e) = copied {
        let _ = std::fs::remove_file(dest);
        return Err(e);
    }
    Ok(())
}

/// Moves the file `src` to `dest` only if nothing is there, and never
/// replaces or writes through what is (a file, a folder, a link), even one
/// that appears meanwhile: it fails with `AlreadyExists` instead. The name is
/// claimed in one step: a hard link, which fails when the name is taken; or,
/// where there are no hard links (FAT, some shares), a new file filled with a
/// copy (copy_to_new). `src` is removed once it's in place. `private`: on
/// Unix, the file is readable by its owner only (0600).
pub(crate) fn move_no_replace(src: &Path, dest: &Path, private: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(src, std::fs::Permissions::from_mode(0o600));
    }
    match std::fs::hard_link(src, dest) {
        Ok(()) => {
            let _ = std::fs::remove_file(src);
            return Ok(());
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(e),
        Err(_) => {}
    }
    copy_to_new(src, dest, private)?;
    let _ = std::fs::remove_file(src);
    Ok(())
}

/// Moves `src` into `dir` under the first of `kept_names` that's free, and
/// returns where it went. Nothing that is there, or appears meanwhile
/// (another sweep keeping a copy of the same name), is replaced or written
/// through (move_no_replace). On Unix the kept copy is readable by its owner
/// only: the folder it goes to may be readable by others.
pub(crate) fn move_to_free_name(src: &Path, dir: &Path, name: &str) -> Option<PathBuf> {
    claim_free_name(dir, name, |dest| move_no_replace(src, dest, true))
}

/// Like move_to_free_name, but `src` stays where it is: a copy is kept.
pub(crate) fn copy_to_free_name(src: &Path, dir: &Path, name: &str) -> Option<PathBuf> {
    claim_free_name(dir, name, |dest| copy_to_new(src, dest, true))
}

/// The first of `kept_names` in `dir` that `place` manages to claim.
pub(crate) fn claim_free_name(dir: &Path, name: &str, place: impl Fn(&Path) -> std::io::Result<()>) -> Option<PathBuf> {
    for candidate in kept_names(name) {
        let dest = dir.join(candidate);
        if std::fs::symlink_metadata(&dest).is_ok() {
            continue;
        }
        match place(&dest) {
            Ok(()) => return Some(dest),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Makes `unsent_edits_dir` if it isn't there; on Unix only its owner may
/// look in it.
pub(crate) fn make_unsent_edits_dir(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Whether the Live Edit copy at `path`, under `under`, has edits that were
/// never uploaded: its content isn't `synced`, the hash of what was last
/// uploaded or downloaded.
pub(crate) fn is_unsent_under(path: &Path, synced: &[u8], under: &Path) -> bool {
    use sha2::{Digest, Sha256};
    path.starts_with(under)
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
        && std::fs::read(path).is_ok_and(|c| Sha256::digest(&c).as_slice() != synced)
}

/// Before Live Edit copies are deleted, the ones under `under` with edits
/// that were never uploaded (their content isn't what was last uploaded, or
/// downloaded) are moved to `unsent_edits_dir`, and the page is told
/// (`live-edit-kept`). The rest are left to be deleted. Returns the folder
/// when anything was kept there.
///
/// `sweep`: the session's temp folder is about to go (its session ended, the
/// profile is locked), so the copies' entries go too. Otherwise (a watcher
/// stopped) the entry stays: the editor may still have the file open, and a
/// save after this is kept at the sweep, not deleted with the folder.
pub(crate) fn keep_unsent_edits(under: &Path, sweep: bool) -> Option<PathBuf> {
    let unsent: Vec<PathBuf> = {
        let mut copies = live_edit_copies().lock().unwrap_or_else(|e| e.into_inner());
        let found: Vec<PathBuf> = copies
            .iter()
            .filter(|(path, synced)| is_unsent_under(path, synced, under))
            .map(|(path, _)| path.clone())
            .collect();
        if sweep {
            copies.retain(|path, _| !path.starts_with(under));
        }
        found
    };
    if unsent.is_empty() {
        return None;
    }
    let folder = unsent_edits_dir()?;
    make_unsent_edits_dir(&folder).ok()?;
    let mut kept = Vec::new();
    for path in unsent {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if let Some(dest) = move_to_free_name(&path, &folder, &name) {
            kept.push(dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name));
        }
    }
    if kept.is_empty() {
        return None;
    }
    report_kept(&folder, &kept);
    Some(folder)
}

/// Logs the copies kept in `folder` and tells the page (`live-edit-kept`).
pub(crate) fn report_kept(folder: &Path, kept: &[String]) {
    crate::applog::write_line("live-edit", &format!("kept {} cop{} with unsent edits in {}", kept.len(), if kept.len() == 1 { "y" } else { "ies" }, folder.display()));
    if let Some(app) = LIVE_EDIT_APP.get() {
        use tauri::Emitter;
        let _ = app.emit("live-edit-kept", serde_json::json!({ "folder": folder.to_string_lossy(), "files": kept }));
    }
}

/// Live Edit copies that have a watcher running, each with when it was last
/// opened again while it ran (None if it wasn't). A copy opened again gets
/// no second watcher, which would upload every save twice: the one running
/// goes on (end_live_edit_watch).
pub(crate) fn live_edit_watchers() -> &'static StdMutex<std::collections::HashMap<PathBuf, Option<std::time::Instant>>> {
    static WATCHERS: std::sync::OnceLock<StdMutex<std::collections::HashMap<PathBuf, Option<std::time::Instant>>>> = std::sync::OnceLock::new();
    WATCHERS.get_or_init(|| StdMutex::new(std::collections::HashMap::new()))
}

pub(crate) fn sha256_of(data: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).to_vec()
}

/// Before the server's copy is written over a Live Edit copy that's already
/// there (the file opened again), the copy is kept in `unsent_edits_dir` if it
/// has edits that never reached the server: its content is neither what was
/// last uploaded or downloaded, nor what the server holds now (`server_hash`).
/// It's copied, not moved, so the editor that has it open keeps its file, and
/// the page is told. When it can't be kept, nothing is written over it.
pub(crate) fn keep_before_reopen(copy: &Path, server_hash: &[u8]) -> Result<(), String> {
    if !std::fs::symlink_metadata(copy).is_ok_and(|m| m.is_file()) {
        return Ok(());
    }
    let Ok(content) = std::fs::read(copy) else { return Ok(()) };
    let hash = sha256_of(&content);
    let synced = live_edit_copies().lock().unwrap_or_else(|e| e.into_inner()).get(copy).cloned();
    if hash == server_hash || synced.as_deref() == Some(hash.as_slice()) {
        return Ok(());
    }
    let name = copy.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let not_kept = || {
        format!(
            "{name} has changes on this computer that were never uploaded, and they couldn't be kept, so it wasn't opened again. Save it in the editor to upload them, or copy them from {}",
            copy.display()
        )
    };
    let folder = unsent_edits_dir().ok_or_else(not_kept)?;
    make_unsent_edits_dir(&folder).map_err(|_| not_kept())?;
    let dest = copy_to_free_name(copy, &folder, &name).ok_or_else(not_kept)?;
    report_kept(&folder, &[dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name)]);
    Ok(())
}

/// A Live Edit watcher is about to end: idle too long (`idle_end`), the copy
/// gone, the session gone, or the watch itself lost. When the file was opened
/// again meanwhile and the copy is there (`can_go_on`: the end was idleness
/// or the copy missing, which opening it again undoes), it goes on instead,
/// with a new deadline, returned. Otherwise the copy is wiped so the remote
/// contents aren't left lying around in OS temp once editing is done, unless
/// it holds edits that were never uploaded: those are kept, and the user is
/// told. Done under the watchers' lock, so a file opened again either finds
/// this watcher running or none, never one that's ending.
pub(crate) fn end_live_edit_watch(
    app: &tauri::AppHandle,
    session_id: &str,
    filename: &str,
    copy: &Path,
    idle_end: bool,
    can_go_on: bool,
) -> Option<std::time::Instant> {
    use tauri::Emitter;
    let mut watchers = live_edit_watchers().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(Some(opened)) = watchers.get(copy) {
        if can_go_on && opened.elapsed() < LIVE_EDIT_IDLE_LIMIT && std::fs::symlink_metadata(copy).is_ok_and(|m| m.is_file()) {
            let deadline = *opened + LIVE_EDIT_IDLE_LIMIT;
            watchers.insert(copy.to_path_buf(), None);
            return Some(deadline);
        }
    }
    watchers.remove(copy);
    // Errors are intentionally ignored: on Windows the editor may still hold a
    // lock on the file, and the worst case is the file persists until the OS
    // cleans temp.
    let synced = live_edit_copies().lock().unwrap_or_else(|e| e.into_inner()).get(copy).cloned();
    let unsynced = std::fs::read(copy).map(|c| Some(sha256_of(&c)) != synced).unwrap_or(false);
    // Moved out of the temp folder, which a disconnect or a lock wipes.
    let kept_in = keep_unsent_edits(copy, false);
    if unsynced || idle_end {
        let message = if unsynced {
            match &kept_in {
                Some(folder) => format!("{filename} has changes that were not uploaded; they're kept in {}", folder.display()),
                None => format!("{filename} has changes that were not uploaded; the copy on this computer was kept"),
            }
        } else {
            format!("Auto-sync for {filename} stopped after {} hours without a change; open the file again to go on", LIVE_EDIT_IDLE_LIMIT.as_secs() / 3600)
        };
        let _ = app.emit(
            &format!("sftp-sync-status-{}", session_id),
            serde_json::json!({ "status": "error", "message": message }),
        );
    }
    if !unsynced {
        let _ = std::fs::remove_file(copy);
        if let Some(dir) = copy.parent() {
            let _ = std::fs::remove_dir(dir); // the file's own folder, if now empty
        }
    }
    None
}

/// lib.rs's sweep_app_temp, the fork's way: this run's temp root goes
/// (`include_own`, keeping unsent Live Edit copies), and so do the roots
/// other runs left (sweep_old_temp_roots).
pub(crate) fn sweep_temp_roots(include_own: bool) {
    if include_own {
        // Let go of this run's lock first: on Windows a file held open
        // keeps its folder from being removed.
        drop(APP_TEMP_LOCK.lock().unwrap_or_else(|e| e.into_inner()).take());
        remove_own_temp(app_temp_root());
    }
    sweep_old_temp_roots(&std::env::temp_dir(), std::time::Duration::from_secs(24 * 60 * 60));
}

/// Removes this run's temp root, keeping Live Edit copies with edits that
/// never reached the server first, as a disconnect does (keep_unsent_edits).
pub(crate) fn remove_own_temp(root: &Path) {
    keep_unsent_edits(root, true);
    let _ = std::fs::remove_dir_all(root);
}

/// The file in each run's temp root that the run holds locked for as long
/// as it lives (lock_temp_root). The system lets go of it when the process
/// ends, however it ends.
pub(crate) const TEMP_ROOT_LOCK: &str = ".submarine-run.lock";

/// This run's lock on its temp root, taken when the root is made.
pub(crate) static APP_TEMP_LOCK: StdMutex<Option<std::fs::File>> = StdMutex::new(None);

/// Makes the lock file in `root` and locks it; None where that can't be done.
pub(crate) fn lock_temp_root(root: &Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(root.join(TEMP_ROOT_LOCK)).ok()?;
    file.try_lock().ok()?;
    Some(file)
}

/// Removes the temp roots in `temp` that other runs left: ones older than
/// `age` whose run is over. A run that's still going holds its root's lock
/// (another copy of the app beside this one), and its root is left alone;
/// so is one whose lock can't be looked at, or that isn't this user's. A
/// root from before the lock (no lock file) goes by its age only. Live Edit
/// copies in a root are kept in `unsent_edits_dir` before it goes: which of
/// them had edits that never reached the server isn't known any more.
pub(crate) fn sweep_old_temp_roots(temp: &Path, age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(temp) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_app_temp_name(name) {
            continue;
        }
        // The folder itself, never what a link leads to.
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
        if !meta.is_dir() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // SAFETY: geteuid has no preconditions and can't fail.
            if meta.uid() != unsafe { libc::geteuid() } {
                continue;
            }
        }
        let old = meta.modified().map(|t| t.elapsed().map(|a| a > age).unwrap_or(false)).unwrap_or(false);
        if !old {
            continue;
        }
        let lock = match std::fs::OpenOptions::new().read(true).open(e.path().join(TEMP_ROOT_LOCK)) {
            Ok(file) => match file.try_lock() {
                Ok(()) => Some(file),
                // Held: that run is still going. Or locks don't work there,
                // and nothing says it's over.
                Err(_) => continue,
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        keep_live_edit_copies_of(&e.path());
        drop(lock);
        let _ = std::fs::remove_dir_all(e.path());
    }
}

/// The name of a temp root: `submarine-` and its random bytes in hex.
pub(crate) fn app_temp_name(bytes: &[u8]) -> String {
    format!("submarine-{}", hex::encode(bytes))
}

#[cfg(test)]
mod unsent_edits_tests {
    use super::{kept_names, move_to_free_name};

    fn fresh(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("submarine-unsent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_names_a_kept_copy_may_take() {
        let first = |n: &str| kept_names(n).take(3).collect::<Vec<_>>();
        assert_eq!(first("index.php"), ["index.php", "index (2).php", "index (3).php"]);
        assert_eq!(first("Makefile"), ["Makefile", "Makefile (2)", "Makefile (3)"]);
        assert_eq!(first(".bashrc"), [".bashrc", ".bashrc (2)", ".bashrc (3)"]);
    }

    #[test]
    fn a_kept_copy_never_replaces_one_kept_before() {
        let (src_dir, kept) = (fresh("src"), fresh("kept"));
        std::fs::write(kept.join("index.php"), "earlier").unwrap();
        std::fs::write(kept.join("index (2).php"), "earlier 2").unwrap();
        let src = src_dir.join("index.php");
        std::fs::write(&src, "mine").unwrap();
        let dest = move_to_free_name(&src, &kept, "index.php").unwrap();
        assert_eq!(dest, kept.join("index (3).php"));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(kept.join("index.php")).unwrap(), "earlier");
        assert_eq!(std::fs::read_to_string(kept.join("index (2).php")).unwrap(), "earlier 2");
        assert!(!src.exists(), "moved, not copied");
        let _ = std::fs::remove_dir_all(&src_dir);
        let _ = std::fs::remove_dir_all(&kept);
    }

    /// The copy is moved out of the app's private temp folder into Downloads,
    /// which others on the computer may be able to read.
    #[cfg(unix)]
    #[test]
    fn a_kept_copy_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (src_dir, kept) = (fresh("src3"), fresh("kept3"));
        let src = src_dir.join("secret.conf");
        std::fs::write(&src, "mine").unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o644)).unwrap();
        let dest = move_to_free_name(&src, &kept, "secret.conf").unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o600);
        // The copy made where there are no hard links is private from the start.
        let other = src_dir.join("other.conf");
        std::fs::write(&other, "x").unwrap();
        super::copy_to_new(&other, &kept.join("copied.conf"), true).unwrap();
        assert_eq!(std::fs::metadata(kept.join("copied.conf")).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(
            super::copy_to_new(&other, &kept.join("copied.conf"), true).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists,
            "a copy never replaces a file that's there"
        );
        super::make_unsent_edits_dir(&kept.join("folder")).unwrap();
        assert_eq!(std::fs::metadata(kept.join("folder")).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&src_dir);
        let _ = std::fs::remove_dir_all(&kept);
    }

    /// Opening a file again writes the server's copy over the Live Edit copy.
    /// One with edits that never reached the server is kept first, and stays
    /// where it is for the editor that has it open; one without isn't.
    #[test]
    fn opening_a_file_again_keeps_unsent_edits_first() {
        let dir = fresh("reopen");
        let copy = dir.join("reopen-edited.txt");
        std::fs::write(&copy, "changed here").unwrap();
        let downloaded = super::sha256_of(b"as downloaded");
        super::live_edit_copies().lock().unwrap().insert(copy.clone(), downloaded.clone());
        let kept_dir = super::unsent_edits_dir().unwrap();
        // Unchanged since the download, or already what the server holds: nothing to keep.
        let clean = dir.join("reopen-clean.txt");
        std::fs::write(&clean, "as downloaded").unwrap();
        super::live_edit_copies().lock().unwrap().insert(clean.clone(), downloaded.clone());
        super::keep_before_reopen(&clean, &super::sha256_of(b"newer on the server")).unwrap();
        super::keep_before_reopen(&copy, &super::sha256_of(b"changed here")).unwrap();
        assert!(!kept_dir.join("reopen-clean.txt").exists());
        assert!(!kept_dir.join("reopen-edited.txt").exists());
        // Edited, and the server has something else: kept.
        super::keep_before_reopen(&copy, &super::sha256_of(b"newer on the server")).unwrap();
        assert_eq!(std::fs::read_to_string(kept_dir.join("reopen-edited.txt")).unwrap(), "changed here");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "changed here", "the editor's file stays");
        // No copy there yet: nothing to do.
        super::keep_before_reopen(&dir.join("reopen-new.txt"), &downloaded).unwrap();
        {
            let mut copies = super::live_edit_copies().lock().unwrap();
            copies.remove(&copy);
            copies.remove(&clean);
        }
        let _ = std::fs::remove_file(kept_dir.join("reopen-edited.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another run's temp root that's still in use (its lock is held) is
    /// left alone; one whose run is over goes, its Live Edit copies kept first.
    #[test]
    fn the_startup_sweep_leaves_running_copies_alone_and_keeps_live_edit_copies() {
        let temp = fresh("sweep");
        let pid = std::process::id();
        let busy = temp.join(super::app_temp_name(&[1; 12]));
        let done = temp.join(super::app_temp_name(&[2; 12]));
        for root in [&busy, &done] {
            std::fs::create_dir_all(root.join("sftp_session-1/0123456789abcdef")).unwrap();
            std::fs::create_dir_all(root.join("drag_session-1")).unwrap();
            std::fs::write(root.join("drag_session-1/dragged.bin"), "x").unwrap();
        }
        let busy_copy = busy.join(format!("sftp_session-1/0123456789abcdef/sweep-busy-{pid}.txt"));
        let done_copy = done.join(format!("sftp_session-1/0123456789abcdef/sweep-done-{pid}.txt"));
        std::fs::write(&busy_copy, "busy").unwrap();
        std::fs::write(&done_copy, "edited").unwrap();
        let held = super::lock_temp_root(&busy).expect("a lock");
        // The other run's lock file, no longer held by anyone.
        drop(super::lock_temp_root(&done).expect("a lock"));
        super::sweep_old_temp_roots(&temp, std::time::Duration::ZERO);
        assert!(busy_copy.exists(), "a run that's still going keeps its folder");
        assert!(!done.exists(), "a finished run's folder is removed");
        let kept_dir = super::unsent_edits_dir().unwrap();
        assert_eq!(std::fs::read_to_string(kept_dir.join(format!("sweep-done-{pid}.txt"))).unwrap(), "edited");
        assert!(!kept_dir.join("dragged.bin").exists(), "drag copies aren't edits");
        drop(held);
        let _ = std::fs::remove_file(kept_dir.join(format!("sweep-done-{pid}.txt")));
        let _ = std::fs::remove_dir_all(&temp);
    }

    /// Quitting the app removes its temp folder; a Live Edit copy in it with
    /// edits that never reached the server is kept first, and one without
    /// goes with the folder.
    #[test]
    fn quitting_keeps_unsent_edits() {
        use sha2::{Digest, Sha256};
        let root = fresh("exit-root");
        let edited = root.join("sftp_session-1/0123456789abcdef/exit-edited.txt");
        let clean = root.join("sftp_session-1/fedcba9876543210/exit-clean.txt");
        for (path, content) in [(&edited, "changed here"), (&clean, "as on the server")] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        {
            let mut copies = super::live_edit_copies().lock().unwrap();
            copies.insert(edited.clone(), Sha256::digest(b"as on the server").to_vec());
            copies.insert(clean.clone(), Sha256::digest(b"as on the server").to_vec());
        }
        super::remove_own_temp(&root);
        assert!(!root.exists(), "the temp folder is gone");
        let kept_dir = super::unsent_edits_dir().unwrap();
        assert_eq!(std::fs::read_to_string(kept_dir.join("exit-edited.txt")).unwrap(), "changed here");
        assert!(!kept_dir.join("exit-clean.txt").exists(), "a copy without edits isn't kept");
        assert!(!super::live_edit_copies().lock().unwrap().keys().any(|p| p.starts_with(&root)));
        let _ = std::fs::remove_file(kept_dir.join("exit-edited.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_where_the_copy_would_go_is_not_written_through() {
        let (src_dir, kept, elsewhere) = (fresh("src2"), fresh("kept2"), fresh("elsewhere"));
        let target = elsewhere.join("victim");
        std::os::unix::fs::symlink(&target, kept.join("notes.txt")).unwrap();
        let src = src_dir.join("notes.txt");
        std::fs::write(&src, "mine").unwrap();
        let dest = move_to_free_name(&src, &kept, "notes.txt").unwrap();
        assert_eq!(dest, kept.join("notes (2).txt"));
        assert!(!target.exists(), "nothing was written where the link leads");
        for d in [src_dir, kept, elsewhere] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// Wipes a session's temp folders (Live Edit copies, drag staging), keeping
/// unsent edits first (keep_unsent_edits).
pub(crate) fn sweep_session_temp(session_id: &str) {
    for dir in [session_sftp_dir(session_id), session_drag_dir(session_id)].into_iter().flatten() {
        if dir.exists() {
            keep_unsent_edits(&dir, true);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Moves every Live Edit copy in another run's temp root
/// (`sftp_<session>/<hash>/<file>`) to `unsent_edits_dir`, and says so.
/// Only real folders and regular files: a link is never followed.
pub(crate) fn keep_live_edit_copies_of(root: &Path) {
    let real_dirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|rd| rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    let Some(folder) = unsent_edits_dir() else { return };
    let mut kept = Vec::new();
    for session in real_dirs(root) {
        if !session.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("sftp_")) {
            continue;
        }
        for copy_dir in real_dirs(&session) {
            let Ok(files) = std::fs::read_dir(&copy_dir) else { continue };
            for file in files.flatten() {
                if !file.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                if kept.is_empty() && make_unsent_edits_dir(&folder).is_err() {
                    return;
                }
                let name = file.file_name().to_string_lossy().into_owned();
                if let Some(dest) = move_to_free_name(&file.path(), &folder, &name) {
                    kept.push(dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name));
                }
            }
        }
    }
    if !kept.is_empty() {
        report_kept(&folder, &kept);
    }
}

#[cfg(test)]
mod temp_root_tests {
    /// The startup sweep recognises the temp roots this app makes (it used
    /// to look for 32 hex digits where 24 are made, and swept nothing), and
    /// nothing else.
    #[test]
    fn the_startup_sweep_knows_its_own_temp_folders() {
        use crate::{app_temp_name, is_app_temp_name};
        assert!(is_app_temp_name(&app_temp_name(&[0xab; 12])));
        assert!(is_app_temp_name(&format!("submarine-{}", "0f".repeat(16))));
        for other in ["submarine-", "submarine-xyz", "submarine-liveedit-test-1", "submarine-0123456789abcdef0123456", "other-abababababababababababab", "submarine-abababababababababababab/x"] {
            assert!(!is_app_temp_name(other), "{other}");
        }
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
                ("", "mod live_edit;", 1),
                ("", "use live_edit::*;", 1),
                ("disconnect_session", "sweep_session_temp(", 1),
                ("teardown_connection_key", "sweep_session_temp(", 1),
                ("teardown_profile", "sweep_session_temp(", 1),
                ("sweep_app_temp", "sweep_temp_roots(include_own)", 1),
                ("app_temp_root", "app_temp_name(&bytes)", 1),
                ("app_temp_root", "APP_TEMP_LOCK.lock()", 1),
                ("app_temp_root", "lock_temp_root(&root)", 1),
                ("sftp_open_remote_file", "live_edit_copies(", 3),
                ("sftp_open_remote_file", "live_edit_watchers(", 2),
                ("sftp_open_remote_file", "sha256_of(", 2),
                ("sftp_open_remote_file", "keep_before_reopen(", 1),
                ("sftp_open_remote_file", "end_live_edit_watch(", 1),
                ("sftp_open_remote_file", "LIVE_EDIT_APP", 1),
            ],
        );
    }
}

//! A profile a restore or an import makes (lib.rs, import_shared_profile and
//! restore_personal_profile): made only where there isn't one yet
//! (setup_master_db_with's `must_create`), and when the restore fails, removed
//! only if this call made it (discard_half_built_profile).

use crate::DbState;
use std::path::{Path, PathBuf};

/// The half-built profile's file, if this call made it. The callers open
/// with `must_create`, so it's this call's when the profile was opened
/// (`opened`), or when opening got as far as setting it up here (its last
/// step, the first save, failed). A profile that another window made or
/// opened meanwhile stays, and so does one open in another window now.
/// Asked before discard_half_built_profile clears the open profile's state.
pub(crate) fn made_here(app: &tauri::AppHandle, state: &DbState, name: &str, opened: Option<u64>) -> Option<PathBuf> {
    let path = crate::profile_path(app, name).ok();
    let made_here = opened.is_some()
        || path.is_some() && state.db_path.lock().ok().is_some_and(|g| g.as_ref() == path.as_ref());
    path.filter(|_| made_here)
}

/// Removes the half-built profile at `p`, with its one-window lock (#204).
pub(crate) fn remove_profile_file(state: &DbState, p: &Path) {
    let lock_path = p.with_extension("submarine.lock");
    // Its one-window lock (#204) goes with it, or the name would stay "open in
    // another window" in this one until it quit. Not held here any more
    // (another profile opened here since): only when no other window has it.
    let ours = state
        .profile_lock
        .lock()
        .ok()
        .and_then(|mut held| if held.as_ref().is_some_and(|(locked, _)| *locked == *p) { held.take() } else { None });
    let lock = match ours {
        Some((_, file)) => crate::ProfileLock::New(file),
        None => match crate::try_lock_file(&lock_path) {
            Ok(lock) => lock,
            Err(_) => return,
        },
    };
    let _ = std::fs::remove_file(p);
    drop(lock);
    let _ = std::fs::remove_file(lock_path);
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the line that brings the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod profile_create;", 1),
                ("discard_half_built_profile", "profile_create::made_here(app, &state, name, opened)", 1),
                ("discard_half_built_profile", "profile_create::remove_profile_file(&state, &p)", 1),
            ],
        );
    }
}

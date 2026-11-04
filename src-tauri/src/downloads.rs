//! Finishing a download: what's there already is replaced only when asked,
//! names that differ only in case are kept apart, and nothing lands in a
//! folder this system runs programs from at login (`LoginFolders`).
//!
//! The fork's own, moved here out of lib.rs (#136) so a sync with the
//! original project doesn't conflict on it. lib.rs reaches it through
//! `use downloads::*`.

use crate::*;

/// Puts a finished download at `dest`: over what's there when the user chose
/// to replace it (an atomic rename), and otherwise only if nothing is there,
/// failing with `AlreadyExists` if something is (move_no_replace). That also
/// catches a second download of the same name that finished first, and on a
/// file system that ignores case, a file whose name differs only in case.
pub(crate) async fn finalize_download(temp: &std::path::Path, dest: &std::path::Path, replace: bool) -> std::io::Result<()> {
    if replace {
        return tokio::fs::rename(temp, dest).await;
    }
    let (temp, dest) = (temp.to_path_buf(), dest.to_path_buf());
    tokio::task::spawn_blocking(move || move_no_replace(&temp, &dest, false))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

/// Leaves out the files of a folder download whose path, ignoring case, is
/// one an earlier file has, when `ignores_case` (the local file system treats
/// them as one name). Returns how many were left out.
pub(crate) fn drop_case_twins(files: &mut Vec<(String, String, u64)>, ignores_case: bool) -> u64 {
    if !ignores_case {
        return 0;
    }
    let before = files.len();
    let mut seen = std::collections::HashSet::new();
    files.retain(|(_, rel, _)| seen.insert(rel.to_lowercase()));
    (before - files.len()) as u64
}

/// The note on a finished folder-download card for files left out because
/// another file's name differs from theirs only in case (drop_case_twins).
pub(crate) fn case_twins_note(skipped: u64) -> Option<String> {
    match skipped {
        0 => None,
        1 => Some("1 file skipped: its name differs from another file's only in upper and lower case, which this computer treats as the same name".into()),
        n => Some(format!("{} files skipped: their names differ from other files' only in upper and lower case, which this computer treats as the same name", n)),
    }
}

/// The note on a finished folder-download card for files left out because a
/// file of that name was saved there meanwhile (finalize_download).
pub(crate) fn existing_skipped_note(skipped: u64) -> Option<String> {
    match skipped {
        0 => None,
        1 => Some("1 file skipped: a file of that name was already there, and was left as it was".into()),
        n => Some(format!("{} files skipped: files of those names were already there, and were left as they were", n)),
    }
}

#[cfg(test)]
mod download_finalize_tests {
    use super::{drop_case_twins, finalize_download};

    fn fresh(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("submarine-finalize-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Two downloads to the same name: the one that finishes second doesn't
    /// replace the first unless the user chose to replace it.
    #[tokio::test]
    async fn a_finished_download_replaces_nothing_unless_asked_to() {
        let dir = fresh("one");
        let dest = dir.join("report.txt");
        let (first, second) = (dir.join("report.txt.sub-tmp-1"), dir.join("report.txt.sub-tmp-2"));
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        finalize_download(&first, &dest, false).await.unwrap();
        assert!(!first.exists(), "the temp file is gone once it's in place");
        let err = finalize_download(&second, &dest, false).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "first");
        finalize_download(&second, &dest, true).await.unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "second", "replaced when asked to");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_in_the_way_is_not_written_through() {
        let dir = fresh("link");
        let target = dir.join("elsewhere");
        std::os::unix::fs::symlink(&target, dir.join("x.txt")).unwrap();
        let temp = dir.join("x.txt.sub-tmp-1");
        std::fs::write(&temp, "data").unwrap();
        assert_eq!(finalize_download(&temp, &dir.join("x.txt"), false).await.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_that_differ_only_in_case_are_one_name_where_case_is_ignored() {
        let list = || {
            ["Docs/Readme.md", "docs/README.md", "docs/other.md", "Makefile", "makefile"]
                .iter()
                .map(|rel| (format!("/srv/{rel}"), rel.to_string(), 1))
                .collect::<Vec<_>>()
        };
        let mut files = list();
        assert_eq!(drop_case_twins(&mut files, true), 2);
        let kept: Vec<&str> = files.iter().map(|(_, rel, _)| rel.as_str()).collect();
        assert_eq!(kept, ["Docs/Readme.md", "docs/other.md", "Makefile"]);
        let mut files = list();
        assert_eq!(drop_case_twins(&mut files, false), 0, "where case counts, they're different files");
        assert_eq!(files.len(), 5);
        assert!(super::case_twins_note(2).unwrap().starts_with("2 files skipped"));
        assert_eq!(super::existing_skipped_note(0), None);
    }
}

/// The folders this system runs programs from at login, for this user and
/// for all users.
pub(crate) fn autorun_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut out = Vec::new();
    if cfg!(windows) {
        for var in ["APPDATA", "ProgramData"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs\Startup"));
            }
        }
    } else if let Some(home) = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf()) {
        if cfg!(target_os = "macos") {
            out.push(home.join("Library/LaunchAgents"));
            out.push("/Library/LaunchAgents".into());
            out.push("/Library/LaunchDaemons".into());
        } else {
            let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
            out.push(config.join("autostart"));
            out.push("/etc/xdg/autostart".into());
        }
    }
    out
}

/// Whether `path` is one of `dirs` or inside one. Compared as text (the file
/// doesn't exist yet), with Windows' verbatim prefix and separators evened
/// out, and without regard to case where the file system ignores it.
pub(crate) fn lands_in(path: &std::path::Path, dirs: &[std::path::PathBuf]) -> bool {
    let norm = |p: &std::path::Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.strip_prefix("//?/").unwrap_or(&s).trim_end_matches('/').to_string();
        if cfg!(any(windows, target_os = "macos")) { s.to_lowercase() } else { s }
    };
    let p = norm(path);
    dirs.iter().map(|d| norm(d)).any(|d| !d.is_empty() && (p == d || p.starts_with(&format!("{}/", d))))
}

/// The folders this system runs programs from at login (autorun_dirs), as
/// written and as resolved, for asking whether a file a server named may be
/// written somewhere. Every writer that puts files the server names below a
/// folder the user chose asks it: folder downloads, and a mirror's downloads
/// (mirror.rs).
pub(crate) struct LoginFolders(Vec<std::path::PathBuf>);

impl LoginFolders {
    pub(crate) fn new() -> Self {
        Self::of(autorun_dirs())
    }

    pub(crate) fn of(dirs: Vec<std::path::PathBuf>) -> Self {
        let resolved: Vec<std::path::PathBuf> = dirs.iter().filter_map(|d| d.canonicalize().ok()).collect();
        Self(dirs.into_iter().chain(resolved).collect())
    }

    /// Whether a file a server named may be written at `path`: not in or
    /// under a login folder. Checked as written and, for the part of the path
    /// that exists, as resolved, so a link or junction to a login folder
    /// (`~/Downloads/x -> ~/.config/autostart`) doesn't get around it.
    pub(crate) fn may_hold_server_file(&self, path: &std::path::Path) -> bool {
        !lands_in(path, &self.0) && !resolve_existing_part(path).is_some_and(|p| lands_in(&p, &self.0))
    }
}

/// `path` with the longest part of it that exists resolved (links followed,
/// `..` taken out), and the rest, which doesn't exist yet, as written. None
/// when nothing of it resolves.
pub(crate) fn resolve_existing_part(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut rest = Vec::new();
    let mut part = path;
    loop {
        if let Ok(mut real) = part.canonicalize() {
            real.extend(rest.iter().rev());
            return Some(real);
        }
        rest.push(part.file_name()?.to_os_string());
        part = part.parent()?;
    }
}

/// The note on a finished folder-download card for files left out because
/// they would land in a login folder (see autorun_dirs).
pub(crate) fn autorun_skipped_note(skipped: u64) -> Option<String> {
    match skipped {
        0 => None,
        1 => Some("1 file skipped: it would land in a folder this computer runs programs from at login".into()),
        n => Some(format!("{} files skipped: they would land in a folder this computer runs programs from at login", n)),
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
                ("", "mod downloads;", 1),
                ("", "use downloads::*;", 1),
                ("sftp_download_file", "finalize_download(", 1),
                ("download_dir_file", "finalize_download(", 1),
                ("sftp_download_dir", "LoginFolders", 1),
                ("sftp_download_dir", "drop_case_twins(", 1),
                ("sftp_download_dir", "autorun_skipped_note(", 1),
                ("sftp_download_dir", "case_twins_note(", 1),
                ("sftp_download_dir", "existing_skipped_note(", 1),
            ],
        );
    }
}

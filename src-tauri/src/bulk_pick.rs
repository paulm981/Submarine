//! Bulk push (#159): picking the files to send, several at once, in the
//! system's own dialog. The page then sends each with `sftp_upload_file`,
//! which checks the path itself (`guard_local_path`).
//!
//! The fork's own, in a file of its own so a sync with the original project
//! doesn't conflict on it; lib.rs registers the command in one line.

/// Most files one pick takes: past this, send their folder instead.
static MAX_FILES: usize = 1000;

/// A file picked to send: its full path, its name and its size.
#[derive(serde::Serialize, Debug, PartialEq)]
pub struct PickedFile {
    path: String,
    name: String,
    size: u64,
}

/// The files picked, in the order the dialog gave them. Empty when the
/// dialog was closed without picking.
#[tauri::command]
pub async fn bulk_pick_files() -> Result<Vec<PickedFile>, String> {
    #[cfg(mobile)]
    {
        Err("Picking files isn't available on a phone or tablet.".into())
    }
    #[cfg(desktop)]
    {
        let picked = rfd::AsyncFileDialog::new().set_title("Choose the files to send").pick_files().await;
        let paths: Vec<std::path::PathBuf> = picked.unwrap_or_default().iter().map(|f| f.path().to_path_buf()).collect();
        tokio::task::spawn_blocking(move || describe(&paths)).await.map_err(|e| e.to_string())?
    }
}

/// Each path's name and size, refusing more than `MAX_FILES` and anything
/// that isn't a file (a dialog can hand back a folder on some systems).
fn describe(paths: &[std::path::PathBuf]) -> Result<Vec<PickedFile>, String> {
    if paths.len() > MAX_FILES {
        return Err(format!("Pick at most {MAX_FILES} files at once, or send the folder they're in."));
    }
    paths
        .iter()
        .map(|p| {
            let meta = std::fs::metadata(p).map_err(|e| format!("Can't read {}: {e}", p.display()))?;
            if !meta.is_file() {
                return Err(format!("{} isn't a file. Use Choose a folder… for a folder.", p.display()));
            }
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Ok(PickedFile { path: p.to_string_lossy().into_owned(), name, size: meta.len() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, emptied first.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("submarine-bulk-pick-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn describes_each_file_in_order() {
        let dir = scratch("order");
        let (a, b) = (dir.join("b.txt"), dir.join("a.bin"));
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"").unwrap();
        let got = describe(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(
            got,
            vec![
                PickedFile { path: a.to_string_lossy().into_owned(), name: "b.txt".into(), size: 5 },
                PickedFile { path: b.to_string_lossy().into_owned(), name: "a.bin".into(), size: 0 },
            ]
        );
        assert_eq!(describe(&[]).unwrap(), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_a_folder_a_missing_file_and_too_many() {
        let dir = scratch("refuse");
        assert!(describe(&[dir.clone()]).unwrap_err().contains("isn't a file"));
        assert!(describe(&[dir.join("gone")]).unwrap_err().starts_with("Can't read"));
        let many = vec![dir.join("x"); MAX_FILES + 1];
        assert!(describe(&many).unwrap_err().contains("at most 1000"));
        let _ = std::fs::remove_dir_all(&dir);
    }
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
                ("", "mod bulk_pick;", 1),
                ("run", "bulk_pick::bulk_pick_files", 1),
            ],
        );
    }
}

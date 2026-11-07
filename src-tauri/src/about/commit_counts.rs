//! How far a build made from source is behind the newest release (`commit_counts`).
//!
//! The fork's own, moved here out of about.rs (#136) so a sync with the
//! original project doesn't conflict on it. about.rs brings it back in with
//! `use commit_counts::*`.

use super::*;

impl UpdateInfo {
    pub(crate) fn none(current: String) -> Self {
        UpdateInfo {
            current,
            latest: None,
            has_update: false,
            release_url: None,
            main_ahead: None,
            main_url: None,
            upstream_ahead: None,
            upstream_url: None,
        }
    }
}

/// The commits the fork's main has that this build doesn't (for a build made
/// from source), and — when asked — the commits the original project's main
/// has that the fork's doesn't. Best effort, like the rest: None when GitHub
/// couldn't tell, or it wasn't asked.
pub(crate) async fn commit_counts(client: &reqwest::Client, watch_upstream: bool) -> (Option<u64>, Option<u64>) {
    let upstream_owner = UPSTREAM_REPO.split('/').next().unwrap_or(UPSTREAM_REPO);
    let upstream_head = format!("{}:main", upstream_owner);
    let main = async {
        if BUILD_COMMIT.is_empty() {
            None
        } else {
            ahead_by(client, BUILD_COMMIT, "main").await
        }
    };
    let upstream = async {
        if watch_upstream {
            ahead_by(client, "main", &upstream_head).await
        } else {
            None
        }
    };
    tokio::join!(main, upstream)
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "about.rs",
            include_str!("../about.rs"),
            &[
                ("", "mod commit_counts;", 1),
                ("", "pub(crate) use commit_counts::*;", 1),
            ],
        );
    }
}

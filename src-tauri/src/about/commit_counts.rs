//! How far a build made from source is behind the newest release (`commit_counts`),
//! and where a fork release sorts (`fork_release_order`).
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

/// Where a fork release sorts against a version with the same numbers (the
/// pre-release step of about.rs's `semver_cmp`, one hook line there):
/// `-fork.N` comes after its base and that base's betas, in order of N, as
/// the fork's releases always have. "0.5.1" < "0.5.1-fork.3" <
/// "0.5.1-fork.10" < "0.5.2". Without it, semver would put "0.5.1-fork.3"
/// below "0.5.1", which a build from source of a clone without tags says
/// (Cargo's version). None when neither is a fork release: semver's own
/// order holds.
pub(crate) fn fork_release_order(a: Option<&[String]>, b: Option<&[String]>) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering::{Greater, Less};
    let fork_n = |pre: Option<&[String]>| match pre {
        Some([word, n]) if word == "fork" => n.parse::<u64>().ok(),
        _ => None,
    };
    match (fork_n(a), fork_n(b)) {
        (None, None) => None,
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        (Some(_), None) => Some(Greater),
        (None, Some(_)) => Some(Less),
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
            "about.rs",
            include_str!("../about.rs"),
            &[
                ("", "mod commit_counts;", 1),
                ("", "pub(crate) use commit_counts::*;", 1),
                ("check_for_updates", "commit_counts(&client, watch_upstream)", 1),
                ("semver_cmp", "fork_release_order(", 1),
                // Not calls into this file, but the fork's other lines in
                // about.rs's update check: running_version is build.rs's
                // whole version, and both settings reach the check.
                ("running_version", "=> VERSION,", 1),
                ("check_for_updates", "include_beta: Option<bool>, watch_upstream: Option<bool>", 1),
                ("check_for_updates", "latest_release(&client, &url, current, include_beta)", 1),
                ("latest_release", "newest_release(releases, include_beta)", 1),
            ],
        );
    }
}

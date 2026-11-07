//! The fork's own tests, kept in a file of their own (#136) so a sync with
//! the original project doesn't conflict on them.

use super::{newest_release, semver_greater, GhRelease};

fn release(tag: &str, prerelease: bool) -> GhRelease {
    GhRelease { tag_name: tag.into(), html_url: format!("page/{tag}"), draft: false, prerelease }
}

#[test]
fn the_newest_fork_release_wins_and_latest_build_never_counts() {
    let releases = vec![
        release("latest-main", true),
        release("v0.3.8-fork.2", false),
        release("v0.3.8-fork.10", false),
        release("v0.3.8-fork.9", false),
    ];
    assert_eq!(newest_release(releases), Some(("0.3.8-fork.10".into(), "page/v0.3.8-fork.10".into())));
    assert_eq!(newest_release(vec![release("latest-main", true)]), None);
    // A build up to 244 says "0.3.8": the first fork release is newer.
    assert!(semver_greater("0.3.8-fork.1", "0.3.8"));
}

#[test]
fn fork_releases_come_after_their_version_in_order() {
    assert!(semver_greater("0.3.8-fork.1", "0.3.8"));
    assert!(semver_greater("0.3.8-fork.2", "0.3.8-fork.1"));
    assert!(semver_greater("0.3.8-fork.10", "0.3.8-fork.9"));
    assert!(semver_greater("0.3.9", "0.3.8-fork.5"));
    // The count starts again with the original project's next version.
    assert!(semver_greater("0.4.0-fork.1", "0.3.8-fork.10"));
    assert!(!semver_greater("0.3.8-fork.10", "0.4.0-fork.1"));
    assert!(!semver_greater("0.3.8-fork.1", "0.3.8-fork.1"));
    assert!(!semver_greater("0.3.8", "0.3.8-fork.1"));
    // Other suffixes still sort equal to their base.
    assert!(!semver_greater("0.3.8-beta", "0.3.8"));
    assert!(!semver_greater("0.3.8", "0.3.8-beta"));
    assert!(semver_greater("0.10.0", "0.9.9"));
}

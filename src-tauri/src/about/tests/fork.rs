//! The fork's own tests in `about`'s `tests`, kept in a file of their own
//! (#136) so a sync with the original project doesn't conflict on them.

use crate::about::{newest_release, running_version, semver_greater, GhRelease, VERSION};

fn release(tag: &str, prerelease: bool) -> GhRelease {
    GhRelease { tag_name: tag.into(), html_url: format!("page/{tag}"), draft: false, prerelease }
}

#[test]
fn the_newest_fork_release_wins_and_latest_build_never_counts() {
    let releases = || {
        vec![
            release("latest-main", true),
            release("v0.3.8-fork.2", false),
            release("v0.3.8-fork.10", false),
            release("v0.3.8-fork.9", false),
        ]
    };
    let newest = Some(("0.3.8-fork.10".into(), "page/v0.3.8-fork.10".into()));
    assert_eq!(newest_release(releases(), false), newest);
    // With "Tell me about beta versions" on, the old "Latest build" is read
    // but sorts below every numbered release.
    assert_eq!(newest_release(releases(), true), newest);
    assert_eq!(newest_release(vec![release("latest-main", true)], false), None);
    // A build up to 244 says "0.3.8": the first fork release is newer.
    assert!(semver_greater("0.3.8-fork.1", "0.3.8"));
}

// The fork's releases are published as full releases, not pre-releases
// (build.yml), so the owner is told about each one with the beta setting off
// as well as on (#283).
#[test]
fn a_fork_release_is_offered_with_the_beta_setting_off() {
    let releases = || vec![release("v0.5.1-fork.2", false), release("v0.5.1-fork.3", false), release("v0.5.2-beta.1", true)];
    let fork3 = Some(("0.5.1-fork.3".into(), "page/v0.5.1-fork.3".into()));
    assert_eq!(newest_release(releases(), false), fork3);
    let (latest, _) = newest_release(releases(), false).unwrap();
    assert!(semver_greater(&latest, "0.5.1-fork.2"));
    assert!(semver_greater(&latest, "0.5.1"));
    // With it on, a beta of the next version is newer still.
    assert_eq!(newest_release(releases(), true), Some(("0.5.2-beta.1".into(), "page/v0.5.2-beta.1".into())));
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
    // 0.5.1: a build from source that says plain "0.5.1" is older than any
    // fork release of 0.5.1, and the count goes by number, not by text.
    assert!(semver_greater("0.5.1-fork.3", "0.5.1"));
    assert!(!semver_greater("0.5.1", "0.5.1-fork.3"));
    assert!(semver_greater("0.5.1-fork.10", "0.5.1-fork.9"));
    assert!(semver_greater("0.5.1-fork.1", "0.5.0-fork.16"));
    assert!(!semver_greater("0.5.1-fork.1", "0.5.2"));
    // A fork release is newer than the original project's betas of its
    // version; their betas still sort as semver says.
    assert!(semver_greater("0.5.1-fork.1", "0.5.1-beta.2"));
    assert!(!semver_greater("0.5.1-beta.2", "0.5.1-fork.1"));
    assert!(semver_greater("0.5.1", "0.5.1-beta.2"));
    assert!(semver_greater("0.10.0", "0.9.9"));
}

#[test]
fn the_running_version_is_the_forks() {
    // build.rs's whole version (a release's tag, or the newest fork tag on a
    // build from source), not Cargo's: Windows builds carry "0.5.1" only.
    assert_eq!(running_version(), VERSION);
}

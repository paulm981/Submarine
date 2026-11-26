//! Which files a mirror pushes, with their mode, once a burst of saves has
//! settled; and how many patterns a mirror may carry.
//!
//! The fork's own, moved here out of mirror.rs (#136) so a sync with the
//! original project doesn't conflict on it. mirror.rs brings it back in with
//! `use settle::*`.

use super::*;

/// Whether attributes are a regular file's. russh-sftp's `is_regular` only
/// asks whether the regular-file bit is among the type bits, which a link's
/// type (0o120000) and a socket's (0o140000) also have.
pub(crate) fn is_plain_file(attrs: &FileAttributes) -> bool {
    attrs.permissions.is_some_and(|p| p & 0o170000 == 0o100000)
}

/// The permissions a pushed file takes over from the one it replaces:
/// without setuid and setgid, which would run it as whoever owned the old
/// file.
pub(crate) fn pushed_mode(old: u32) -> u32 {
    old & 0o1777
}

/// What a pushed file is given before it's closed, through its open handle
/// (FSETSTAT), never by path: by the time the upload ends, a link to
/// another file can be in the temporary file's place, and a change by path
/// would land on that file.
#[derive(Clone, Copy, Default)]
pub(crate) struct Settle {
    /// The replaced file's owner and group; best effort.
    pub(crate) owner: Option<(u32, u32)>,
    /// The exact permissions. Creating the file went through the server's
    /// umask, so it's no more open than this already.
    pub(crate) mode: Option<u32>,
    /// Whether failing to set `mode` fails the upload.
    pub(crate) mode_required: bool,
}

/// At most this many patterns in each of a mirror's lists ("Only these",
/// "Skip these"), and at most this many characters in each. A saved mirror
/// is part of its server's record and syncs with a shared profile, so its
/// lists come from whoever else writes to that profile: every pattern is
/// a regex checked against every path a scan meets (#288).
pub(crate) static MAX_PATTERNS: usize = 100;
pub(crate) static MAX_PATTERN_CHARS: usize = 512;

/// Refuses a mirror whose pattern lists are over the bounds, with a message
/// that says which list and what to do. Called where a mirror starts or is
/// previewed, before a pattern is compiled.
pub(crate) fn patterns_in_bounds(spec: &MirrorSpec) -> Result<(), String> {
    for (list, name) in [(&spec.includes, "Only these"), (&spec.excludes, "Skip these")] {
        if list.len() > MAX_PATTERNS {
            return Err(format!(
                "\"{name}\" has {} patterns. A mirror takes at most {MAX_PATTERNS} in each list: remove some, or put them together with * and ?.",
                list.len()
            ));
        }
        if list.iter().any(|p| p.chars().count() > MAX_PATTERN_CHARS) {
            return Err(format!("A pattern in \"{name}\" is longer than {MAX_PATTERN_CHARS} characters. Make it shorter."));
        }
    }
    Ok(())
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "mirror.rs",
            include_str!("../mirror.rs"),
            &[
                ("", "mod settle;", 1),
                ("", "pub(crate) use settle::*;", 1),
                ("start", "patterns_in_bounds(&spec)?;", 1),
                ("dry_run", "patterns_in_bounds(&spec)?;", 1),
            ],
        );
    }
}

#[cfg(test)]
mod pattern_bound_tests {
    use super::*;

    fn spec(includes: Vec<String>, excludes: Vec<String>) -> MirrorSpec {
        MirrorSpec {
            local: String::new(),
            remote: "/srv".into(),
            soft_delete: true,
            excludes,
            includes,
            direction: "both".into(),
            conflict_resolution: "local".into(),
        }
    }

    fn many(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("*.ext{i}")).collect()
    }

    #[test]
    fn lists_at_the_bound_are_taken() {
        let long = "a".repeat(MAX_PATTERN_CHARS);
        let mut full = many(MAX_PATTERNS - 1);
        full.push(long.clone());
        assert_eq!(patterns_in_bounds(&spec(full.clone(), full)), Ok(()));
        // Characters, not bytes: 512 two-byte letters are within it.
        assert_eq!(patterns_in_bounds(&spec(vec!["é".repeat(MAX_PATTERN_CHARS)], vec![])), Ok(()));
        assert_eq!(patterns_in_bounds(&spec(vec![], vec![])), Ok(()));
    }

    #[test]
    fn one_pattern_too_many_is_refused() {
        let only = patterns_in_bounds(&spec(many(MAX_PATTERNS + 1), vec![])).unwrap_err();
        assert!(only.contains("\"Only these\" has 101 patterns"), "{only}");
        let skip = patterns_in_bounds(&spec(vec![], many(MAX_PATTERNS + 1))).unwrap_err();
        assert!(skip.contains("\"Skip these\" has 101 patterns"), "{skip}");
    }

    #[test]
    fn one_character_too_long_is_refused() {
        let long = vec!["a".repeat(MAX_PATTERN_CHARS + 1)];
        let only = patterns_in_bounds(&spec(long.clone(), vec![])).unwrap_err();
        assert!(only.contains("\"Only these\" is longer than 512"), "{only}");
        let skip = patterns_in_bounds(&spec(vec![], long)).unwrap_err();
        assert!(skip.contains("\"Skip these\" is longer than 512"), "{skip}");
    }
}

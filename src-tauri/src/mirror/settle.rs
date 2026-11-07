//! Which files a mirror pushes, with their mode, once a burst of saves has settled.
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
            ],
        );
    }
}

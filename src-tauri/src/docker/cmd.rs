//! The docker command a tab runs, as words (`Cmd`).
//!
//! The fork's own, moved here out of docker.rs (#136) so a sync with the
//! original project doesn't conflict on it. docker.rs brings it back in with
//! `use cmd::*`.

use super::*;

/// A docker command, a word per argument: `docker …`, or compose's words
/// (after `docker compose`, or `docker-compose` where the plugin isn't
/// there). As words, it runs the same through the user's shell and as root.
pub(crate) enum Cmd {
    Docker(Vec<String>),
    Compose(Vec<String>),
}

pub(crate) fn words(first: &[&str], rest: &[String]) -> Vec<String> {
    first.iter().map(|w| w.to_string()).chain(rest.iter().cloned()).collect()
}

/// `docker` and these words.
pub(crate) fn docker(args: &[&str]) -> Cmd {
    Cmd::Docker(words(&["docker"], &args.iter().map(|w| w.to_string()).collect::<Vec<_>>()))
}

impl Cmd {
    /// As the user's shell runs it.
    pub(crate) fn shell(&self) -> String {
        let join = |w: &[String]| w.iter().map(|w| shq(w)).collect::<Vec<_>>().join(" ");
        match self {
            Cmd::Docker(w) => join(w),
            Cmd::Compose(w) => format!("{} {}", compose_prefix(), join(w)),
        }
    }
}

/// The daemon's socket refused the user (it's root's and the docker
/// group's): root may do it. Only that: a daemon that's down, or a
/// "permission denied" from inside a container, isn't root's to fix.
pub(crate) fn daemon_refused(out: &str) -> bool {
    let lower = out.to_lowercase();
    lower.contains("permission denied while trying to connect to the docker")
        || lower.contains("docker.sock: connect: permission denied")
}

// Every call from the original project's code into this file (fork_hooks.rs):
// a sync that drops one, or the lines that bring the file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "docker.rs",
            include_str!("../docker.rs"),
            &[
                ("", "mod cmd;", 1),
                ("", "pub(crate) use cmd::*;", 1),
            ],
        );
    }
}

//! A tab's root access as it was found (`RootAccess`, `RootGate`), and the
//! terminals a tab is opening (`TerminalOpenings`).
//!
//! The fork's own, moved here out of ssh_manager.rs (#136) so a sync with the
//! original project doesn't conflict on it. ssh_manager.rs brings it back in with
//! `use root_state::*`.

use super::*;

/// Which terminals are still opening, so a close that arrives while one is
/// (the page closed the tab before the server answered) isn't lost: opening
/// a shell waits on the server, and the terminal is registered only at the
/// end. Without this, the close found nothing, the open then registered the
/// terminal, and its shell kept running on the server with nobody to see it.
/// As local_term.rs does for Home terminals.
///
/// Each open gets a number. A close forgets the terminal's number, as does a
/// newer open of the same terminal (a reconnect): at the end, an open whose
/// number isn't the terminal's any more closes its channel and registers
/// nothing.
#[derive(Default)]
pub struct TerminalOpenings {
    pub(crate) last: u64,
    pub(crate) opening: HashMap<String, u64>,
}

impl TerminalOpenings {
    /// An open of `terminal_id` starts; keep the number for `finish`.
    pub fn begin(&mut self, terminal_id: &str) -> u64 {
        self.last += 1;
        self.opening.insert(terminal_id.to_string(), self.last);
        self.last
    }

    /// The page closed `terminal_id`: an open still under way is let go.
    pub fn close(&mut self, terminal_id: &str) {
        self.opening.remove(terminal_id);
    }

    /// The open numbered `n` ended (opened or failed). True when the
    /// terminal is still wanted from it: register it, holding this lock
    /// until it's in `terminal_txs` so a close can't come in between.
    pub fn finish(&mut self, terminal_id: &str, n: u64) -> bool {
        if self.opening.get(terminal_id) == Some(&n) {
            self.opening.remove(terminal_id);
            true
        } else {
            false
        }
    }
}

/// How an Info panel action gets root on a tab's server: the tool that let
/// the user in (`sudo` or `dzdo`) and, when it asked for one, the password —
/// in memory only, zeroised on drop, written to the tool's stdin, never on a
/// command line (as for SftpElevation).
#[derive(Clone)]
pub struct RootAccess {
    pub tool: &'static str,
    /// `None` = passwordless (`-n`).
    pub password: Option<zeroize::Zeroizing<String>>,
    /// Through a root shell (`dzdo su -`, …: the words after the tool) with
    /// the command on its stdin, where the tool's rules allow only that.
    /// `None` = the tool runs the command itself.
    pub shell: Option<&'static str>,
    /// When the password last worked. Held while one not known to work
    /// lately is tried, so the tab's other calls wait for that answer
    /// instead of each signing in with a password that may have stopped
    /// working (lib.rs, password_turn).
    pub checked: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    /// Sudo's cached sign-ins (tickets) are neither used nor left behind
    /// (`-k`): the AI's root (#120), which must never ride on a sign-in of
    /// yours, or leave one for anything else.
    pub no_tickets: bool,
}

/// A tab's gate for passwords the user types (SshState::root_gates).
#[derive(Default)]
pub struct RootGate {
    /// A salted digest of the last password sudo or dzdo turned down, and
    /// when.
    pub rejected: Option<([u8; 32], std::time::Instant)>,
}

impl RootAccess {
    pub fn new(
        tool: &'static str,
        password: Option<zeroize::Zeroizing<String>>,
        shell: Option<&'static str>,
        checked: Option<std::time::Instant>,
    ) -> Self {
        RootAccess { tool, password, shell, checked: Arc::new(tokio::sync::Mutex::new(checked)), no_tickets: false }
    }

    /// The same, using and leaving no sudo tickets (`no_tickets`).
    pub fn without_tickets(mut self) -> Self {
        self.no_tickets = true;
        self
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
            "ssh_manager.rs",
            include_str!("../ssh_manager.rs"),
            &[
                ("", "mod root_state;", 1),
                ("", "pub(crate) use root_state::*;", 1),
            ],
        );
    }
}

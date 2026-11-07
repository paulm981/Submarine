//! The AI tools Home's menu can start (#117): a list, so that adding one is
//! an entry, not code of its own.
//!
//! Each starts already connected to Submarine's MCP server
//! (`submarine --mcp`, helper.rs), the way that tool takes one: Claude Code
//! with `--mcp-config` (claude.rs), Codex with `-c` settings (codex.rs). The
//! channel's address and token are never on its command line or in a file:
//! the tool passes them from its own environment, where Submarine put them,
//! to the MCP server it starts. And each is told that Submarine's tools need
//! no second prompt: Submarine asks before each command itself (#116).

use super::{claude, codex};
use std::path::{Path, PathBuf};

/// The MCP server's name in every tool's settings. Not plain "submarine",
/// which a server of your own could be called: Claude Code's rule for every
/// tool of this server would let that one through too.
pub(crate) const SERVER: &str = "submarine-ssh-bridge";

enum Kind {
    Claude,
    Codex,
}

/// An AI tool in the list.
pub(crate) struct Tool {
    /// The id Home's menu names it by.
    pub id: &'static str,
    pub label: &'static str,
    /// The name its tabs go by in the AI panel ("Claude", "Claude 2").
    pub short: &'static str,
    kind: Kind,
}

/// The built-in AI tools, in Home's order.
pub(crate) const TOOLS: &[Tool] = &[
    Tool { id: super::CLAUDE_ID, label: "Claude Code", short: "Claude", kind: Kind::Claude },
    Tool { id: "ai:codex", label: "Codex", short: "Codex", kind: Kind::Codex },
];

pub(crate) fn by_id(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

impl Tool {
    /// Where it's installed, if it is.
    pub(crate) fn find(&self) -> Option<PathBuf> {
        match self.kind {
            Kind::Claude => claude::find(),
            Kind::Codex => codex::find(),
        }
    }

    /// The tool at `program`, connected to `helper`. `data_dir`: where a
    /// settings file goes, for a tool that needs one.
    pub(crate) fn launch(&self, program: &Path, helper: &Path, data_dir: Option<&Path>) -> Result<Launch, String> {
        match self.kind {
            Kind::Claude => claude::launch(program, helper, data_dir),
            Kind::Codex => codex::launch(program, helper),
        }
    }
}

/// What Home starts: a program and its arguments, what to add to its
/// environment besides the address and the token, and what to take out.
pub(crate) struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<&'static str>,
}

/// The first of `places` that is a file, then `names` in each folder of
/// PATH. An app started from Finder or the Start menu may not have your
/// shell's PATH, so the usual places are given by name.
pub(crate) fn find_program(mut places: Vec<PathBuf>, names: &[&str]) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            places.extend(names.iter().map(|n| dir.join(n)));
        }
    }
    places.into_iter().find(|p| p.is_absolute() && p.is_file())
}

/// Your home folder, as `find_program`'s places start from it.
pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// PATH with `program`'s own folder first. An npm install is a script run
/// by node, which is often next to it (nvm, Homebrew), and an app started
/// from the desktop may not have that folder in its PATH.
pub(crate) fn path_with(program: &Path) -> Vec<(String, String)> {
    let Some(dir) = program.parent() else { return Vec::new() };
    let mut paths = vec![dir.to_path_buf()];
    if let Some(p) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&p));
    }
    std::env::join_paths(paths).map(|joined| vec![("PATH".to_string(), joined.to_string_lossy().into_owned())]).unwrap_or_default()
}

/// Whether `program` is a `.cmd` or `.bat` wrapper (an npm install on
/// Windows), which runs through cmd.exe.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_cmd(program: &Path) -> bool {
    program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// A `.cmd` wrapper, run through cmd.exe: `cmd /d /c call "<wrapper>"
/// <args>`. `call` keeps cmd from taking the first quote off. cmd.exe reads
/// `%`, `^`, `&`, `|`, `<`, `>`, `!` and `"` even in quoted paths, so any of
/// them in the wrapper's path or an argument is refused rather than passed.
#[cfg(windows)]
pub(crate) fn through_cmd(program: &Path, args: Vec<String>, label: &str) -> Result<(PathBuf, Vec<String>), String> {
    let program = program.to_string_lossy().into_owned();
    if !cmd_safe(&program) || !args.iter().all(|a| cmd_safe(a)) {
        return Err(format!("{label}'s path, or Submarine's, has a character cmd.exe can't pass on (% ^ & | < > ! \"). Install {label} with its own installer (an .exe) instead."));
    }
    let cmd = crate::local_open::system32().join("cmd.exe");
    let mut all = vec!["/d".to_string(), "/c".to_string(), "call".to_string(), program];
    all.extend(args);
    Ok((cmd, all))
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn cmd_safe(s: &str) -> bool {
    !s.chars().any(|c| "%^&|<>!\"".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tool_has_its_own_id_and_name() {
        assert_eq!(TOOLS.len(), 2);
        for (i, t) in TOOLS.iter().enumerate() {
            assert!(t.id.starts_with("ai:"));
            assert!(TOOLS[i + 1..].iter().all(|o| o.id != t.id && o.short != t.short), "{}", t.id);
            assert!(std::ptr::eq(by_id(t.id).unwrap(), t));
        }
        assert_eq!(by_id(super::super::CLAUDE_ID).unwrap().label, "Claude Code");
        assert_eq!(by_id("ai:codex").unwrap().label, "Codex");
        assert!(by_id("ai:other").is_none());
    }

    #[test]
    fn the_server_name_is_its_own() {
        // Not a name a server of your own is likely to have, and nothing a
        // tool would change in its tools' names (Claude Code changes only
        // `.` and spaces, Codex keeps it as a bare TOML key).
        assert_ne!(SERVER, "submarine");
        assert!(SERVER.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'));
    }

    #[test]
    fn cmd_exe_specials_are_refused() {
        assert!(cmd_safe(r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"));
        for bad in [r"C:\100%\claude.cmd", r"C:\a&b\claude.cmd", r"C:\x^y\c.cmd", "C:\\q\"\\c.cmd", r"C:\hi!\c.cmd"] {
            assert!(!cmd_safe(bad), "{bad}");
        }
        assert!(is_cmd(Path::new(r"C:\npm\codex.CMD")) && is_cmd(Path::new("x.bat")) && !is_cmd(Path::new("codex.exe")));
    }

    #[test]
    fn a_program_is_found_in_its_place() {
        let dir = std::env::temp_dir().join(format!("submarine-find-tool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let there = dir.join("tool");
        std::fs::write(&there, b"").unwrap();
        assert_eq!(find_program(vec![dir.join("none"), there.clone()], &[]), Some(there.clone()));
        // A folder by that name isn't the program.
        assert_eq!(find_program(vec![dir.clone()], &[]), None);
        assert_eq!(find_program(vec![PathBuf::from("tool")], &[]), None, "only a full path");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

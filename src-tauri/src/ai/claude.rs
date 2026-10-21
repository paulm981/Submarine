//! Claude Code, the AI tool in Home's menu (#32, Decision 5): where it's
//! installed, and how it starts already connected (Decision 1).
//!
//! It starts with `--mcp-config` naming one MCP server, `submarine`, which
//! runs this program with `--mcp` (helper.rs). That adds to your own MCP
//! servers rather than replacing them. The entry's `env` names
//! `${SUBMARINE_AI_ADDR}` and `${SUBMARINE_AI_TOKEN}`, which Claude Code fills
//! in from its own environment, where Submarine put them: the token is never
//! on a command line or in a file.
//!
//! - **macOS and Linux:** the settings go on the command line as JSON.
//! - **Windows:** `claude` may be a `.cmd` wrapper, which runs through
//!   cmd.exe, and JSON doesn't survive cmd.exe's quoting. So `--mcp-config`
//!   names a file in Submarine's data folder instead. It holds no secret.

use super::channel::{ENV_ADDR, ENV_TOKEN};
use std::path::{Path, PathBuf};

pub(crate) const LABEL: &str = "Claude Code";
/// The name its tabs go by in the AI panel ("Claude", "Claude 2").
pub(crate) const SHORT: &str = "Claude";

/// The MCP settings Claude Code is given: Submarine's helper, by full path.
pub(crate) fn mcp_config(helper: &Path) -> serde_json::Value {
    serde_json::json!({
        "mcpServers": {
            "submarine": {
                "type": "stdio",
                "command": helper.to_string_lossy(),
                "args": ["--mcp"],
                "env": {
                    ENV_ADDR: format!("${{{ENV_ADDR}}}"),
                    ENV_TOKEN: format!("${{{ENV_TOKEN}}}"),
                }
            }
        }
    })
}

/// Where Claude Code is installed, if it is: its native installer's place
/// first, then npm's, Homebrew's, then PATH. An app started from Finder or
/// the Start menu may not have your shell's PATH, so the usual places are
/// looked in by name.
pub(crate) fn find() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut places: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Some(home) = env("USERPROFILE") {
            places.push(home.join(r".local\bin\claude.exe"));
        }
        if let Some(appdata) = env("APPDATA") {
            places.push(appdata.join(r"npm\claude.cmd"));
        }
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                places.push(dir.join("claude.exe"));
                places.push(dir.join("claude.cmd"));
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = env("HOME") {
            places.push(home.join(".local/bin/claude"));
            places.push(home.join(".claude/local/claude"));
            places.push(home.join(".npm-global/bin/claude"));
        }
        places.push(PathBuf::from("/opt/homebrew/bin/claude"));
        places.push(PathBuf::from("/usr/local/bin/claude"));
        if let Some(path) = std::env::var_os("PATH") {
            places.extend(std::env::split_paths(&path).map(|d| d.join("claude")));
        }
    }
    places.into_iter().find(|p| p.is_absolute() && p.is_file())
}

/// What Home starts: a program and its arguments, and what to add to its
/// environment besides the address and the token.
pub(crate) struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Claude Code at `claude`, with Submarine's MCP settings. `data_dir`:
/// where the Windows settings file goes.
pub(crate) fn launch(claude: &Path, helper: &Path, data_dir: Option<&Path>) -> Result<Launch, String> {
    let config = mcp_config(helper);
    let mut env = Vec::new();
    // An npm install is a script run by node: node is often next to it
    // (nvm, Homebrew), and an app started from the desktop may not have that
    // folder in its PATH.
    if let Some(dir) = claude.parent() {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(p) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&p));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            env.push(("PATH".to_string(), joined.to_string_lossy().into_owned()));
        }
    }
    #[cfg(windows)]
    {
        let dir = data_dir.ok_or("Couldn't find Submarine's data folder.")?.join("ai");
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't write Claude Code's settings: {e}"))?;
        let file = dir.join("claude-mcp.json");
        let body = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(&file, body).map_err(|e| format!("Couldn't write Claude Code's settings: {e}"))?;
        let file = file.to_string_lossy().into_owned();
        let is_cmd = claude.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
        if is_cmd {
            return cmd_launch(claude, &file, env);
        }
        Ok(Launch { program: claude.to_path_buf(), args: vec!["--mcp-config".into(), file], env })
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        Ok(Launch { program: claude.to_path_buf(), args: vec!["--mcp-config".into(), config.to_string()], env })
    }
}

/// A `.cmd` wrapper runs through cmd.exe: `cmd /d /c call "<claude.cmd>"
/// --mcp-config "<file>"`. `call` keeps cmd from taking the first quote
/// off. cmd.exe reads `%`, `^`, `&`, `|`, `<`, `>`, `!` and `"` even in
/// paths, so a path with any of them is refused rather than passed.
#[cfg(windows)]
fn cmd_launch(claude: &Path, file: &str, env: Vec<(String, String)>) -> Result<Launch, String> {
    let claude = claude.to_string_lossy().into_owned();
    if !cmd_safe(&claude) || !cmd_safe(file) {
        return Err("Claude Code's path has a character cmd.exe can't pass on (% ^ & | < > ! \"). Install Claude Code with its own installer (claude.exe) instead.".into());
    }
    let cmd = crate::local_open::system32().join("cmd.exe");
    Ok(Launch { program: cmd, args: vec!["/d".into(), "/c".into(), "call".into(), claude, "--mcp-config".into(), file.to_string()], env })
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn cmd_safe(s: &str) -> bool {
    !s.chars().any(|c| "%^&|<>!\"".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_names_the_helper_and_no_secret() {
        let c = mcp_config(Path::new("/Applications/Submarine.app/Contents/MacOS/submarine"));
        let s = &c["mcpServers"]["submarine"];
        assert_eq!(s["command"], "/Applications/Submarine.app/Contents/MacOS/submarine");
        assert_eq!(s["args"], serde_json::json!(["--mcp"]));
        assert_eq!(s["env"][ENV_TOKEN], "${SUBMARINE_AI_TOKEN}");
        assert_eq!(s["env"][ENV_ADDR], "${SUBMARINE_AI_ADDR}");
    }

    #[cfg(not(windows))]
    #[test]
    fn on_macos_and_linux_the_settings_go_on_the_command_line() {
        let l = launch(Path::new("/home/me/.local/bin/claude"), Path::new("/usr/bin/submarine"), None).unwrap();
        assert_eq!(l.program, PathBuf::from("/home/me/.local/bin/claude"));
        assert_eq!(l.args[0], "--mcp-config");
        let v: serde_json::Value = serde_json::from_str(&l.args[1]).unwrap();
        assert_eq!(v, mcp_config(Path::new("/usr/bin/submarine")));
        assert!(l.env.iter().any(|(k, v)| k == "PATH" && v.starts_with("/home/me/.local/bin")));
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_settings_go_in_a_file() {
        let dir = std::env::temp_dir().join(format!("submarine-claude-test-{}", std::process::id()));
        let l = launch(Path::new(r"C:\Users\me\.local\bin\claude.exe"), Path::new(r"C:\Program Files\Submarine\submarine.exe"), Some(&dir)).unwrap();
        assert_eq!(l.args[0], "--mcp-config");
        let body: serde_json::Value = serde_json::from_slice(&std::fs::read(&l.args[1]).unwrap()).unwrap();
        assert_eq!(body, mcp_config(Path::new(r"C:\Program Files\Submarine\submarine.exe")));
        let l = launch(Path::new(r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"), Path::new(r"C:\s\submarine.exe"), Some(&dir)).unwrap();
        assert!(l.program.ends_with("cmd.exe"));
        assert_eq!(&l.args[..4], &["/d", "/c", "call", r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cmd_exe_specials_are_refused() {
        assert!(cmd_safe(r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"));
        for bad in [r"C:\100%\claude.cmd", r"C:\a&b\claude.cmd", r"C:\x^y\c.cmd", "C:\\q\"\\c.cmd", r"C:\hi!\c.cmd"] {
            assert!(!cmd_safe(bad), "{bad}");
        }
    }
}

//! Claude Code, the AI tool in Home's menu (#32, Decision 5): where it's
//! installed, and how it starts already connected (Decision 1).
//!
//! It starts with `--mcp-config` naming one MCP server, `submarine`, which
//! runs this program with `--mcp` (helper.rs). That adds to your own MCP
//! servers rather than replacing them. The entry's `env` names
//! `${SUBMARINE_AI_ADDR}`, `${SUBMARINE_AI_TOKEN}` and `${SUBMARINE_AI_PID}`
//! (the app's process id, which the helper checks the channel by), which Claude Code fills
//! in from its own environment, where Submarine put them: the token is never
//! on a command line or in a file.
//!
//! It also starts with `--allowedTools mcp__submarine__*`, so Claude Code
//! doesn't ask before it uses Submarine's tools: Submarine asks before each
//! command, and asking twice is one too many (#116). Claude Code still asks
//! before anything it does on this computer.
//!
//! - **macOS and Linux:** the settings go on the command line as JSON.
//! - **Windows:** `claude` may be a `.cmd` wrapper, which runs through
//!   cmd.exe, and JSON doesn't survive cmd.exe's quoting. So `--mcp-config`
//!   names a file in Submarine's data folder instead. It holds no secret.

use super::channel::{ENV_ADDR, ENV_PID, ENV_TOKEN};
use std::path::{Path, PathBuf};

pub(crate) const LABEL: &str = "Claude Code";
/// The name its tabs go by in the AI panel ("Claude", "Claude 2").
pub(crate) const SHORT: &str = "Claude";
/// Claude Code's rule for every tool of the `submarine` MCP server, which it
/// then uses without asking. Claude Code's flag takes several values, so it
/// goes last, after `--mcp-config`'s.
const ALLOW_SUBMARINE: [&str; 2] = ["--allowedTools", "mcp__submarine__*"];

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
                    ENV_PID: format!("${{{ENV_PID}}}"),
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

/// What Home starts: a program and its arguments, what to add to its
/// environment besides the address and the token, and what to take out.
pub(crate) struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<&'static str>,
}

/// The variables Claude Code logs in with when it's set to use a cloud
/// rather than Anthropic: `CLAUDE_CODE_USE_*`, and the prefix of the
/// credentials that cloud reads.
const CLOUD_LOGINS: &[(&str, &str)] = &[
    ("CLAUDE_CODE_USE_BEDROCK", "AWS_"),
    ("CLAUDE_CODE_USE_MANTLE", "AWS_"),
    ("CLAUDE_CODE_USE_ANTHROPIC_AWS", "AWS_"),
    ("CLAUDE_CODE_USE_VERTEX", "GOOGLE_"),
    ("CLAUDE_CODE_USE_FOUNDRY", "AZURE_"),
];

/// What Claude Code starts without (super::CREDENTIAL_ENV), less the
/// cloud's variables it logs in with, if it does. `var`: the environment.
fn env_remove(var: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    let on = |k: &str| var(k).is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"));
    let kept: Vec<&str> = CLOUD_LOGINS.iter().filter(|(switch, _)| on(switch)).map(|(_, prefix)| *prefix).collect();
    super::CREDENTIAL_ENV.iter().copied().filter(|name| !kept.iter().any(|p| name.starts_with(p))).collect()
}

/// Claude Code at `claude`, with Submarine's MCP settings. `data_dir`:
/// where the Windows settings file goes.
pub(crate) fn launch(claude: &Path, helper: &Path, data_dir: Option<&Path>) -> Result<Launch, String> {
    let config = mcp_config(helper);
    let mut env = Vec::new();
    let env_remove = env_remove(|k| std::env::var(k).ok());
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
            return cmd_launch(claude, &file, env, env_remove);
        }
        Ok(Launch { program: claude.to_path_buf(), args: with_allowed(vec!["--mcp-config".into(), file]), env, env_remove })
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        Ok(Launch { program: claude.to_path_buf(), args: with_allowed(vec!["--mcp-config".into(), config.to_string()]), env, env_remove })
    }
}

fn with_allowed(mut args: Vec<String>) -> Vec<String> {
    args.extend(ALLOW_SUBMARINE.map(String::from));
    args
}

/// A `.cmd` wrapper runs through cmd.exe: `cmd /d /c call "<claude.cmd>"
/// --mcp-config "<file>" --allowedTools mcp__submarine__*`. `call` keeps
/// cmd from taking the first quote off. cmd.exe reads `%`, `^`, `&`, `|`,
/// `<`, `>`, `!` and `"` even in paths, so a path with any of them is
/// refused rather than passed. The `*` means nothing to cmd.exe, and the
/// wrapper passes it on as it is.
#[cfg(windows)]
fn cmd_launch(claude: &Path, file: &str, env: Vec<(String, String)>, env_remove: Vec<&'static str>) -> Result<Launch, String> {
    let claude = claude.to_string_lossy().into_owned();
    if !cmd_safe(&claude) || !cmd_safe(file) {
        return Err("Claude Code's path has a character cmd.exe can't pass on (% ^ & | < > ! \"). Install Claude Code with its own installer (claude.exe) instead.".into());
    }
    let cmd = crate::local_open::system32().join("cmd.exe");
    let args = vec!["/d".into(), "/c".into(), "call".into(), claude, "--mcp-config".into(), file.to_string()];
    Ok(Launch { program: cmd, args: with_allowed(args), env, env_remove })
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
        assert_eq!(s["env"][ENV_PID], "${SUBMARINE_AI_PID}");
    }

    #[cfg(not(windows))]
    #[test]
    fn on_macos_and_linux_the_settings_go_on_the_command_line() {
        let l = launch(Path::new("/home/me/.local/bin/claude"), Path::new("/usr/bin/submarine"), None).unwrap();
        assert_eq!(l.program, PathBuf::from("/home/me/.local/bin/claude"));
        assert_eq!(l.args.len(), 4);
        assert_eq!(l.args[0], "--mcp-config");
        let v: serde_json::Value = serde_json::from_str(&l.args[1]).unwrap();
        assert_eq!(v, mcp_config(Path::new("/usr/bin/submarine")));
        assert_eq!(&l.args[2..], &["--allowedTools", "mcp__submarine__*"]);
        assert!(l.env.iter().any(|(k, v)| k == "PATH" && v.starts_with("/home/me/.local/bin")));
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_settings_go_in_a_file() {
        let dir = std::env::temp_dir().join(format!("submarine-claude-test-{}", std::process::id()));
        let l = launch(Path::new(r"C:\Users\me\.local\bin\claude.exe"), Path::new(r"C:\Program Files\Submarine\submarine.exe"), Some(&dir)).unwrap();
        assert_eq!(l.args.len(), 4);
        assert_eq!(l.args[0], "--mcp-config");
        let body: serde_json::Value = serde_json::from_slice(&std::fs::read(&l.args[1]).unwrap()).unwrap();
        assert_eq!(body, mcp_config(Path::new(r"C:\Program Files\Submarine\submarine.exe")));
        assert_eq!(&l.args[2..], &["--allowedTools", "mcp__submarine__*"]);
        let l = launch(Path::new(r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"), Path::new(r"C:\s\submarine.exe"), Some(&dir)).unwrap();
        assert!(l.program.ends_with("cmd.exe"));
        assert_eq!(&l.args[..4], &["/d", "/c", "call", r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"]);
        assert_eq!(l.args[4], "--mcp-config");
        assert_eq!(&l.args[6..], &["--allowedTools", "mcp__submarine__*"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn it_starts_without_your_ssh_agent_or_cloud_logins() {
        // SSH_AUTH_SOCK is set to where no agent is instead (start_tab).
        assert!(!super::super::CREDENTIAL_ENV.contains(&"SSH_AUTH_SOCK"));
        for name in ["SSH_AGENT_PID", "AWS_SECRET_ACCESS_KEY", "AWS_PROFILE", "AZURE_CLIENT_SECRET", "GOOGLE_APPLICATION_CREDENTIALS", "GITHUB_TOKEN", "GH_TOKEN"] {
            assert!(super::super::CREDENTIAL_ENV.contains(&name), "{name}");
        }
        assert_eq!(env_remove(|_| None), super::super::CREDENTIAL_ENV);
        #[cfg(not(windows))]
        {
            let l = launch(Path::new("/home/me/.local/bin/claude"), Path::new("/usr/bin/submarine"), None).unwrap();
            let without = env_remove(|k| std::env::var(k).ok());
            assert_eq!(l.env_remove, without);
            assert!(l.env_remove.contains(&"SSH_AGENT_PID"));
        }
        // What it needs to work stays.
        for name in ["PATH", "HOME", "USERPROFILE", "HTTPS_PROXY", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"] {
            assert!(!super::super::CREDENTIAL_ENV.contains(&name), "{name}");
        }
    }

    #[test]
    fn a_cloud_claude_code_logs_in_with_keeps_its_variables() {
        let bedrock = env_remove(|k| (k == "CLAUDE_CODE_USE_BEDROCK").then(|| "1".to_string()));
        assert!(!bedrock.iter().any(|n| n.starts_with("AWS_")));
        assert!(bedrock.contains(&"SSH_AGENT_PID") && bedrock.contains(&"AZURE_CLIENT_SECRET") && bedrock.contains(&"GOOGLE_APPLICATION_CREDENTIALS"));
        let vertex = env_remove(|k| (k == "CLAUDE_CODE_USE_VERTEX").then(|| "true".to_string()));
        assert!(!vertex.contains(&"GOOGLE_APPLICATION_CREDENTIALS") && vertex.contains(&"AWS_PROFILE"));
        let foundry = env_remove(|k| (k == "CLAUDE_CODE_USE_FOUNDRY").then(|| "1".to_string()));
        assert!(!foundry.contains(&"AZURE_CLIENT_SECRET") && foundry.contains(&"GH_TOKEN"));
        // Off is off.
        assert_eq!(env_remove(|k| (k == "CLAUDE_CODE_USE_BEDROCK").then(|| "0".to_string())), super::super::CREDENTIAL_ENV);
    }

    #[test]
    fn cmd_exe_specials_are_refused() {
        assert!(cmd_safe(r"C:\Users\Paul M\AppData\Roaming\npm\claude.cmd"));
        for bad in [r"C:\100%\claude.cmd", r"C:\a&b\claude.cmd", r"C:\x^y\c.cmd", "C:\\q\"\\c.cmd", r"C:\hi!\c.cmd"] {
            assert!(!cmd_safe(bad), "{bad}");
        }
        // What's added after the paths passes cmd.exe as it is.
        assert!(ALLOW_SUBMARINE.iter().all(|a| cmd_safe(a) && !a.contains(' ')));
    }

    #[test]
    fn claude_code_doesnt_ask_before_submarines_tools() {
        // Its rule for every tool of one MCP server, named as in mcp_config.
        assert_eq!(ALLOW_SUBMARINE[1], "mcp__submarine__*");
        assert!(mcp_config(Path::new("/x"))["mcpServers"].get("submarine").is_some());
    }
}

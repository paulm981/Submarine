//! Codex, OpenAI's coding agent, as an AI tool in Home's menu (#117): where
//! it's installed, and how it starts already connected.
//!
//! It starts with `-c` settings, which Codex reads as TOML over its own
//! config.toml, adding one MCP server, SERVER, which runs this program with
//! `--mcp` (helper.rs). Your own MCP servers stay as they are, and nothing
//! is written to your settings.
//!
//! - **`env_vars`** names the channel's address, token and Submarine's
//!   process id. Codex gives an MCP server only a few variables of its own
//!   (PATH, HOME and the like) plus those named here, taken from its own
//!   environment, where Submarine put them: the token is never on a command
//!   line or in a file.
//! - **`default_tools_approval_mode = 'approve'`**: Codex doesn't ask before
//!   it uses Submarine's tools. Submarine asks before each command, and
//!   asking twice is one too many (#116). Codex still asks before what it
//!   does on this computer, as you've set it to.
//!
//! The values are TOML literal strings (`'…'`), which keep a Windows path's
//! backslashes as they are and need no `"`, so they pass cmd.exe when Codex
//! is an npm install's `codex.cmd`.
//!
//! Codex's ChatGPT login is OpenAI's own Codex program, unchanged, signing
//! in as you; Submarine only starts it.

use super::channel::{ENV_ADDR, ENV_PID, ENV_TOKEN};
use super::tools::{self, Launch, SERVER};
use std::path::{Path, PathBuf};

/// Where Codex is installed, if it is: npm's place, Homebrew's, then PATH.
pub(crate) fn find() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
            places.push(appdata.join(r"npm\codex.cmd"));
        }
        tools::find_program(places, &["codex.exe", "codex.cmd"])
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = tools::home() {
            places.push(home.join(".local/bin/codex"));
            places.push(home.join(".npm-global/bin/codex"));
        }
        places.push(PathBuf::from("/opt/homebrew/bin/codex"));
        places.push(PathBuf::from("/usr/local/bin/codex"));
        tools::find_program(places, &["codex"])
    }
}

/// The `-c` settings that give Codex Submarine's MCP server at `helper`.
pub(crate) fn settings(helper: &Path) -> Result<Vec<String>, String> {
    let helper = helper.to_string_lossy();
    // A TOML literal string can't hold a `'` or a line break.
    if helper.contains(['\'', '\n', '\r']) {
        return Err("Submarine's path has a ' in it, which Codex's settings can't hold. Move Submarine to a folder without one.".into());
    }
    let set = |key: &str, value: String| ["-c".to_string(), format!("mcp_servers.{SERVER}.{key}={value}")];
    Ok([
        set("command", format!("'{helper}'")),
        set("args", "['--mcp']".into()),
        set("env_vars", format!("['{ENV_ADDR}', '{ENV_TOKEN}', '{ENV_PID}']")),
        set("default_tools_approval_mode", "'approve'".into()),
    ]
    .concat())
}

/// Codex at `codex`, connected to Submarine's MCP server at `helper`.
pub(crate) fn launch(codex: &Path, helper: &Path) -> Result<Launch, String> {
    let args = settings(helper)?;
    let env = tools::path_with(codex);
    // Codex signs in with its own login (~/.codex) or OPENAI_API_KEY, neither
    // of which is taken out.
    let env_remove = super::CREDENTIAL_ENV.to_vec();
    #[cfg(windows)]
    if tools::is_cmd(codex) {
        let (program, args) = tools::through_cmd(codex, args, "Codex")?;
        return Ok(Launch { program, args, env, env_remove });
    }
    Ok(Launch { program: codex.to_path_buf(), args, env, env_remove })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_is_given_the_helper_and_no_secret() {
        let s = settings(Path::new("/Applications/Submarine.app/Contents/MacOS/submarine")).unwrap();
        assert_eq!(
            s,
            [
                "-c",
                "mcp_servers.submarine-ssh-bridge.command='/Applications/Submarine.app/Contents/MacOS/submarine'",
                "-c",
                "mcp_servers.submarine-ssh-bridge.args=['--mcp']",
                "-c",
                "mcp_servers.submarine-ssh-bridge.env_vars=['SUBMARINE_AI_ADDR', 'SUBMARINE_AI_TOKEN', 'SUBMARINE_AI_PID']",
                "-c",
                "mcp_servers.submarine-ssh-bridge.default_tools_approval_mode='approve'",
            ]
        );
        // The variables named are the ones the helper reads.
        assert_eq!([ENV_ADDR, ENV_TOKEN, ENV_PID], ["SUBMARINE_AI_ADDR", "SUBMARINE_AI_TOKEN", "SUBMARINE_AI_PID"]);
    }

    #[test]
    fn a_windows_path_stays_as_it_is() {
        let s = settings(Path::new(r"C:\Program Files\Submarine\submarine.exe")).unwrap();
        assert_eq!(s[1], r"mcp_servers.submarine-ssh-bridge.command='C:\Program Files\Submarine\submarine.exe'");
        // Nothing cmd.exe would read, so an npm install's codex.cmd passes it on.
        assert!(s.iter().all(|a| tools::cmd_safe(a)));
    }

    #[test]
    fn a_path_a_literal_string_cant_hold_is_refused() {
        assert!(settings(Path::new("/Users/o'brien/Submarine.app/Contents/MacOS/submarine")).is_err());
        assert!(settings(Path::new("/x\ny/submarine")).is_err());
    }

    #[test]
    fn it_starts_without_your_ssh_agent_or_cloud_logins() {
        let l = launch(Path::new("/home/me/.npm-global/bin/codex"), Path::new("/usr/bin/submarine")).unwrap();
        assert_eq!(l.env_remove, super::super::CREDENTIAL_ENV);
        // Its own login stays.
        assert!(!l.env_remove.contains(&"OPENAI_API_KEY"));
        assert!(l.env.iter().any(|(k, v)| k == "PATH" && v.starts_with("/home/me/.npm-global/bin")));
        #[cfg(not(windows))]
        {
            assert_eq!(l.program, PathBuf::from("/home/me/.npm-global/bin/codex"));
            assert_eq!(l.args, settings(Path::new("/usr/bin/submarine")).unwrap());
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_npm_install_runs_through_cmd_exe() {
        let l = launch(Path::new(r"C:\Users\Paul M\AppData\Roaming\npm\codex.cmd"), Path::new(r"C:\s\submarine.exe")).unwrap();
        assert!(l.program.ends_with("cmd.exe"));
        assert_eq!(&l.args[..4], &["/d", "/c", "call", r"C:\Users\Paul M\AppData\Roaming\npm\codex.cmd"]);
        assert_eq!(&l.args[4..], settings(Path::new(r"C:\s\submarine.exe")).unwrap());
    }
}

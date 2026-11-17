//! The fork's own part of remote_sh (#275): what the fork's code sends to a
//! server's shell besides the original project's `run_script` and the rest
//! of remote_sh.rs, which reads every script with sh on stdin. remote_sh.rs
//! brings it in with `pub(crate) use fork::*`, so the fork's callers say
//! `remote_sh::quote` and the rest as before.
//!
//! * The fork's encoder, for a command line some shell reads as it is: a
//!   root shell (`dzdo su -`) reads its line in root's own shell, and the
//!   root scripts start with one line (`root_ssh.rs`), their password sent
//!   only once they say so. sshd runs that line through the account's login
//!   shell (`$SHELL -c '<command>'`), which may be sh, bash, zsh, fish, csh
//!   or tcsh. So a script never travels as text those shells would parse:
//!   it goes as one word of octal escapes (`\ooo`, only a backslash and
//!   digits), which a POSIX `sh` turns back into the script with `printf`
//!   and runs. csh and tcsh reject a line break inside quotes, and read `!`
//!   as history; neither ever appears in what they see. Arguments go as
//!   single-quoted words (`quote`), the one quoting every one of those
//!   shells reads the same way.
//! * `send_command`, for the few commands that must also work on a server
//!   with no POSIX sh (Windows' OpenSSH runs cmd.exe or PowerShell): the
//!   OS badge's probe, and a Docker command where `sh` isn't found.
//! * `FORK_ALLOWED`: the fork's exec requests, each with its reason, for
//!   the original project's `scripts_reach_a_server_only_through_here`.

use russh::client::Msg;
use russh::Channel;
use tokio::time::Instant;

/// `s` as one shell word: single-quoted, a quote inside closing the word,
/// adding an escaped quote and reopening it.
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Each of `words` as one shell word, separated by spaces.
pub(crate) fn quote_words(words: &[String]) -> String {
    words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
}

/// Each byte of `script` as a `\ooo` escape, which `printf` turns back into
/// the byte. Only backslashes and digits: no `%` for printf, nothing any
/// shell reads as special inside single quotes.
pub(crate) fn octal(script: &str) -> String {
    let mut out = String::with_capacity(script.len() * 4);
    for b in script.bytes() {
        out.push('\\');
        out.push(char::from(b'0' + (b >> 6)));
        out.push(char::from(b'0' + ((b >> 3) & 7)));
        out.push(char::from(b'0' + (b & 7)));
    }
    out
}

/// What decodes and runs the script (the word after it, `$0`) in POSIX sh:
/// no quote, backslash, `!` or line break inside its single quotes.
const DECODE: &str = r#"'eval "$(printf "$0")"'"#;

/// A command that runs `script` (POSIX sh, any number of lines) with `sh`,
/// whatever the login shell is: `env K=V... sh -c <decode> <script> <args>`.
/// The script sees `args` as `"$@"` and `env` as variables; stdin is the
/// command's own, so a password written to it reaches what the script runs.
pub(crate) fn sh_command(env: &[(&str, &str)], script: &str, args: &[String]) -> String {
    sh_command_with("sh", env, script, args)
}

/// sh_command with `sh` at a path of the caller's (`/bin/sh` for a root
/// shell, whose PATH may be anything).
pub(crate) fn sh_command_with(sh: &str, env: &[(&str, &str)], script: &str, args: &[String]) -> String {
    debug_assert!(sh.chars().all(|c| c.is_ascii_alphanumeric() || c == '/'));
    let mut cmd = String::new();
    if !env.is_empty() {
        cmd.push_str("env");
        for (k, v) in env {
            debug_assert!(!k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
            cmd.push(' ');
            cmd.push_str(k);
            cmd.push('=');
            cmd.push_str(&quote(v));
        }
        cmd.push(' ');
    }
    cmd.push_str(sh);
    cmd.push_str(" -c ");
    cmd.push_str(DECODE);
    cmd.push_str(" '");
    cmd.push_str(&octal(script));
    cmd.push('\'');
    if !args.is_empty() {
        cmd.push(' ');
        cmd.push_str(&quote_words(args));
    }
    cmd
}

/// Turns each argument of sh_command_encoded back from its octal, in
/// order, as `"$@"`. `printf x` keeps a line break at an argument's end.
const ARGS_DECODE: &str = r#"n=$#; while [ "$n" -gt 0 ]; do a=$(printf "$1"; printf x); a=${a%x}; shift; set -- "$@" "$a"; n=$((n - 1)); done"#;

/// sh_command_with for arguments that may hold anything (a command the AI
/// wrote, with `!` or line breaks, which csh and tcsh won't take inside
/// quotes): each goes as octal too, and the script gets them back as
/// `"$@"`. Four times as long as a quoted word.
pub(crate) fn sh_command_encoded(sh: &str, env: &[(&str, &str)], script: &str, args: &[String]) -> String {
    let script = format!("{ARGS_DECODE}\n{script}");
    let args: Vec<String> = args.iter().map(|a| octal(a)).collect();
    sh_command_with(sh, env, &script, &args)
}

/// Who reads what `send_command` sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reader {
    /// POSIX sh, whatever the login shell is (`run_script`): every server
    /// with a sh.
    Sh,
    /// The login shell, the line as it is: only for a server with no POSIX
    /// sh (Windows' cmd.exe or PowerShell), where `sh -s` isn't found. The
    /// line is the app's own and fixed (the OS probe, a Docker command made
    /// of checked words), never a user's or the AI's text.
    LoginShell,
}

/// Runs `script` on `channel`, read by `reader`, and closes its stdin, all
/// by `deadline` (remote_sh's `send_by`: past it the channel is closed).
pub(crate) async fn send_command(channel: &Channel<Msg>, reader: Reader, script: &str, deadline: Instant) -> Result<(), String> {
    match reader {
        Reader::Sh => super::run_script(channel, script, deadline).await,
        Reader::LoginShell => {
            let send = async {
                channel.exec(true, script.as_bytes()).await?;
                channel.eof().await
            };
            super::send_by(channel, deadline, send).await
        }
    }
}

/// The fork's exec requests and shell requests (test code left out), for
/// the original project's `scripts_reach_a_server_only_through_here`, which
/// counts every one in the app against its list. Each is `(file, line,
/// times)`, with why its line goes to a shell as it is. `with_fork` is the
/// one hook in that test.
#[cfg(test)]
pub(super) mod allowed {
    pub(crate) const FORK_ALLOWED: &[(&str, &str, usize)] = &[
        // send_command's line for a server with no POSIX sh (Windows): the
        // OS badge's probe (server_os.rs) and docker.rs's retry where `sh`
        // isn't found. Fixed lines of the app's own.
        ("remote_sh/fork.rs", "channel.exec(true, script.as_bytes()).await?;", 1),
        // Root (sudo, dzdo, `dzdo su -`): ROOT_EXEC_SCRIPT and the probes,
        // one line of the fork's encoder (sh_command_encoded, read by sh
        // whatever the login shell is). They run as a command, not on
        // stdin, because their stdin is the password's, and the password
        // goes only once the script says READ (root_feed,
        // exec_probe_with_password): exec_probe_with_password and
        // exec_fed_capture in root_ssh.rs, open_elevated_sftp in lib.rs
        // (then sftp-server's bytes), and root_channel_start for a root
        // stream or terminal (then the user's typing).
        ("root_ssh.rs", "channel.exec(true, cmd.as_bytes()).await.map_err(|e| e.to_string())?;", 2),
        ("root_ssh.rs", "channel.exec(true, command.as_bytes()).await.map_err(|e| e.to_string())?;", 1),
        ("lib.rs", "channel.exec(true, cmd.as_bytes()).await.map_err(|e| e.to_string())?;", 1),
        // Terminals: their stdin is the user's. The fork's terminal starts
        // the kept session's attach line (keep_session.rs) or the original
        // project's login shell line through this one exec.
        ("lib.rs", "Some(command) => channel.exec(true, command.as_bytes()).await,", 1),
        // The AI's command (ai_tabs), run in the account's login shell as
        // the person would type it there.
        ("ai_tabs/exec.rs", "channel.exec(true, command.as_bytes()).await.map_err(|e| format!(\"Couldn't start the command: {e}\"))?;", 1),
    ];

    /// The original project's lines the fork's code has in another form:
    /// the fork's terminal start (above) holds their login shell line.
    pub(crate) const FORK_REPLACED: &[(&str, &str)] =
        &[("lib.rs", "Some(shell) => channel.exec(true, login_shell_command(&shell).as_bytes()).await,")];

    /// `theirs`, the original project's list, without FORK_REPLACED and
    /// with FORK_ALLOWED. A line of theirs the fork replaces must be on
    /// their list, so a sync that changes it fails here.
    pub(crate) fn with_fork(theirs: &[(&'static str, &'static str, usize)]) -> Vec<(&'static str, &'static str, usize)> {
        for (file, line) in FORK_REPLACED {
            assert!(theirs.iter().any(|(f, l, _)| f == file && l == line), "{file}: `{line}` is no longer on their list");
        }
        theirs
            .iter()
            .filter(|(f, l, _)| !FORK_REPLACED.contains(&(*f, *l)))
            .chain(FORK_ALLOWED)
            .copied()
            .collect()
    }
}

/// Whether `login`'s sh runs a `hostname` of its own rather than the one in
/// `bin` (first on `path`): a busybox built to prefer its applets, as on the
/// Ubuntu 22.04 the checks run on. The original project's
/// `the_overview_probe_never_renames_the_host` can't see its stand-in there,
/// so it skips that shell (one hook there), saying why. Any other answer,
/// a failure to ask included, runs the test as it is.
#[cfg(all(test, unix))]
pub(super) fn own_hostname(login: &[&str], path: &str, bin: &std::path::Path) -> bool {
    let Some((program, args)) = login.split_first() else { return false };
    let Ok(out) = std::process::Command::new(program)
        .args(args)
        .arg("sh -c 'command -v hostname'")
        .env("PATH", path)
        .output()
    else {
        return false;
    };
    let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let own = out.status.success() && found != bin.join("hostname").to_string_lossy();
    if own {
        eprintln!("skipped {program}: its sh runs its own hostname ({found}), not the one on PATH");
    }
    own
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The skip in their overview test is taken only when sh can't find the
    /// stand-in `hostname`: found first on PATH, the test runs as it is.
    #[cfg(unix)]
    #[test]
    fn own_hostname_only_when_the_stand_in_cant_be_seen() {
        let bin = std::env::temp_dir().join(format!("sub-own-hostname-{}", std::process::id()));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("hostname"), "#!/bin/sh\necho labhost\n").unwrap();
        std::fs::set_permissions(bin.join("hostname"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let system = std::env::var("PATH").unwrap_or_default();
        let first = format!("{}:{system}", bin.display());
        assert!(!own_hostname(&["sh", "-c"], &first, &bin));
        // Another hostname comes first: what an applet does to the test.
        let last = format!("{system}:{}", bin.display());
        let other = std::process::Command::new("sh").args(["-c", "command -v hostname"]).env("PATH", &system).output().unwrap();
        if other.status.success() {
            assert!(own_hostname(&["sh", "-c"], &last, &bin));
        }
        // A login shell that can't be started: the test runs, and says so if it fails.
        assert!(!own_hostname(&["no-such-shell-here", "-c"], &first, &bin));
        std::fs::remove_dir_all(&bin).unwrap();
    }

    const SCRIPT: &str = "if [ \"$1\" = 'a b' ]; then echo one; fi\nfor w; do printf '%s|' \"$w\"; done\necho \"$SUB_X\" | tr \"\\n\" ' ' ; echo; echo 'done!'\n";

    fn args() -> Vec<String> {
        ["a b", "it's", "$(id)", "!x", "back\\slash", "line\nbreak", "100%"].map(String::from).to_vec()
    }

    #[cfg(unix)]
    const EXPECTED: &str = "one\na b|it's|$(id)|!x|back\\slash|line\nbreak|100%|v w \ndone!\n";

    #[test]
    fn words_are_single_quoted() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote_words(&["a b".into(), "".into()]), "'a b' ''");
        assert_eq!(octal("a'\n%"), r"\141\047\012\045");
    }

    /// What every login shell is given: one line, and nothing csh or tcsh
    /// read inside quotes (a line break) or anywhere (`!`, history). The
    /// script's own words are all octal.
    #[test]
    fn the_command_is_one_line_without_history_marks() {
        let cmd = sh_command(&[("SUB_X", "v w")], SCRIPT, &args()[..3]);
        assert!(!cmd.contains('\n') && !cmd.contains('\r'), "{cmd}");
        assert!(!cmd.contains('!'), "{cmd}");
        let inner = DECODE.trim_matches('\'');
        assert!(!inner.contains(['\'', '\\', '!', '\n']));
        let after = cmd.split_once(&format!("{} '", DECODE)).unwrap().1;
        let word = after.split_once('\'').unwrap().0;
        assert!(word.len() == SCRIPT.len() * 4 && word.chars().all(|c| c == '\\' || c.is_ascii_digit()), "{word}");
    }

    /// The command run by this machine's shells, as sshd runs it
    /// (`<login shell> -c <command>`): sh, and tcsh and csh where installed.
    #[cfg(unix)]
    #[test]
    fn every_login_shell_runs_the_script_as_written() {
        let cmd = sh_command(&[("SUB_X", "v w")], SCRIPT, &args());
        let mut ran = 0;
        for shell in ["sh", "bash", "tcsh", "csh", "zsh"] {
            let Ok(out) = std::process::Command::new(shell).arg("-c").arg(&cmd).output() else {
                continue;
            };
            // `!x` and the line break are arguments: only the POSIX shells
            // take them. csh and tcsh take the rest as written.
            if matches!(shell, "tcsh" | "csh") {
                let safe = sh_command(&[("SUB_X", "v w")], SCRIPT, &args()[..3]);
                let out = std::process::Command::new(shell).arg("-c").arg(&safe).output().unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    "one\na b|it's|$(id)|v w \ndone!\n",
                    "{shell}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            } else {
                assert_eq!(String::from_utf8_lossy(&out.stdout), EXPECTED, "{shell}: {}", String::from_utf8_lossy(&out.stderr));
            }
            ran += 1;
        }
        assert!(ran >= 1);
    }

    /// Arguments of any kind (`!`, line breaks, quotes, one ending with a
    /// line break, an empty one) reach the script as written, through sh,
    /// tcsh and csh alike.
    #[cfg(unix)]
    #[test]
    fn encoded_arguments_reach_the_script_under_any_login_shell() {
        let args: Vec<String> = ["hi!", "line\nbreak", "it's \"q\" $x `y`", "end\n", "", "100%"].map(String::from).to_vec();
        let cmd = sh_command_encoded("/bin/sh", &[], "for w; do printf '[%s]' \"$w\"; done", &args);
        assert!(!cmd.contains(['\n', '!']), "{cmd}");
        let want: String = args.iter().map(|a| format!("[{a}]")).collect();
        let mut ran = 0;
        for shell in ["sh", "tcsh", "csh"] {
            let Ok(out) = std::process::Command::new(shell).arg("-c").arg(&cmd).output() else { continue };
            assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{shell}: {}", String::from_utf8_lossy(&out.stderr));
            ran += 1;
        }
        assert!(ran >= 1);
    }

    /// A password written to the command's stdin reaches what the script runs.
    #[cfg(unix)]
    #[test]
    fn stdin_reaches_the_script() {
        use std::io::Write;
        let cmd = sh_command(&[], "IFS= read -r p; printf '[%s]' \"$p\"", &[]);
        for shell in ["sh", "tcsh"] {
            let Ok(mut child) = std::process::Command::new(shell)
                .arg("-c")
                .arg(&cmd)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
            else {
                continue;
            };
            child.stdin.take().unwrap().write_all(b"pa'ss $x\n").unwrap();
            let out = child.wait_with_output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "[pa'ss $x]", "{shell}");
        }
    }

    // Every hook into this file from the original project's code is where it
    // belongs (fork_hooks.rs): a merge that drops one, putting a command back
    // together by hand, fails here, while the tests above would still pass.
    #[test]
    fn the_hooks_are_in_place() {
        use crate::fork_hooks::assert_hooks;
        assert_hooks(
            "remote_sh.rs",
            include_str!("../remote_sh.rs"),
            &[
                ("", "mod fork;", 1),
                ("", "pub(crate) use fork::*;", 1),
                ("scripts_reach_a_server_only_through_here", "fork::allowed::with_fork(", 1),
                ("the_overview_probe_never_renames_the_host", "fork::own_hostname(", 1),
            ],
        );
        assert_hooks(
            "lib.rs",
            include_str!("../lib.rs"),
            &[
                ("login_shell_command", "remote_sh::quote(", 1),
                ("ssh_iptables_chain", "remote_sh::quote_words(", 1),
                ("ssh_nft_chain", "remote_sh::quote_words(", 1),
            ],
        );
        // Root over SSH, out of lib.rs into a fork file of its own (#136).
        assert_hooks(
            "root_ssh.rs",
            include_str!("../root_ssh.rs"),
            &[
                ("root_probe_command", "remote_sh::sh_command_encoded(", 1),
                ("root_shell_line", "remote_sh::sh_command_with(", 1),
                ("root_exec_command", "remote_sh::sh_command_encoded(", 1),
                ("root_run", "remote_sh::quote_words(", 1),
                ("root_channel_start", "remote_sh::quote_words(", 2),
                ("sftp_probe_command", "remote_sh::sh_command_with(", 1),
                ("find_sftp_server", "remote_sh::sh_command(", 1),
                ("sh_user_command", "remote_sh::sh_command(", 1),
            ],
        );
        // A Windows server's Docker commands: their sh first, the login
        // shell where there is none.
        assert_hooks(
            "docker.rs",
            include_str!("../docker.rs"),
            &[("run_capture", "remote_sh::Reader::LoginShell", 1), ("run_capture_raw", "remote_sh::send_command(", 1)],
        );
        assert_hooks("shell_sftp.rs", include_str!("../shell_sftp.rs"), &[("", "use crate::remote_sh::quote;", 1)]);
    }

    /// The fork's list goes into theirs, and the line it replaces comes out.
    #[test]
    fn the_fork_list_joins_theirs() {
        use super::allowed::{with_fork, FORK_ALLOWED, FORK_REPLACED};
        let theirs: &[(&str, &str, usize)] = &[("a.rs", "x", 1), (FORK_REPLACED[0].0, FORK_REPLACED[0].1, 1)];
        let all = with_fork(theirs);
        assert_eq!(all.len(), 1 + FORK_ALLOWED.len());
        assert_eq!(all[0], ("a.rs", "x", 1));
        assert!(!all.iter().any(|(f, l, _)| (*f, *l) == FORK_REPLACED[0]));
        let gone = std::panic::catch_unwind(|| with_fork(&[("a.rs", "x", 1)]));
        assert!(gone.is_err(), "a replaced line missing from their list is noticed");
    }

    /// A Windows server's line goes to the login shell as it is, then EOF;
    /// any other goes to sh on stdin (run_script).
    #[tokio::test]
    async fn the_login_shell_gets_the_line_only_when_asked() {
        use crate::ssh_test_server::{connect, TestServer};
        use std::sync::Arc;
        let server = TestServer::echoing_stdin();
        let commands = Arc::clone(&server.commands);
        let s = connect(server, |_| {}, crate::build_ssh_client_config()).await.unwrap();
        let soon = || Instant::now() + std::time::Duration::from_secs(30);
        let a = s.channel_open_session().await.unwrap();
        send_command(&a, Reader::LoginShell, "ver", soon()).await.unwrap();
        let b = s.channel_open_session().await.unwrap();
        send_command(&b, Reader::Sh, "id -un", soon()).await.unwrap();
        let read = |mut ch: Channel<Msg>| async move {
            let mut out = Vec::new();
            while let Some(msg) = ch.wait().await {
                match msg {
                    russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
                    russh::ChannelMsg::Close => break,
                    _ => {}
                }
            }
            String::from_utf8_lossy(&out).into_owned()
        };
        assert_eq!(read(a).await, "", "nothing on stdin, only EOF");
        assert_eq!(read(b).await, super::super::whole("id -un"));
        assert_eq!(*commands.lock().unwrap(), vec!["ver".to_string(), super::super::SH_STDIN.to_string()]);
    }
}

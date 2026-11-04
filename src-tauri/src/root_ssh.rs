//! Root over SSH for the Info panel's actions, the AI's commands and SFTP's
//! root mode: sudo or dzdo, without a password first, then with the one
//! typed in Submarine. The fork's own (FORK.md, "Root when you need it"),
//! moved here out of lib.rs (#136) so a sync with the original project
//! doesn't conflict on it. lib.rs reaches it through `use root_ssh::*`.

use crate::*;

// ---------------------------------------------------------------------------
// Root for the Info panel's actions (sudo / dzdo)
// ---------------------------------------------------------------------------
//
// A service action, a kill or a firewall read that needs root goes through
// sudo or dzdo, tried as SFTP root mode tries them (ELEVATION_TOOLS):
// without a password first, then with the one the user types when the tool
// asks. What worked is kept for the tab (SshState::root_access) — or taken
// from the tab's SFTP root mode — so it's asked for once. Commands are fixed
// words plus validated arguments; the password goes to the tool's stdin.

// Every script below is POSIX sh, sent with remote_sh::sh_command so that
// any login shell (csh and tcsh included) passes it to `sh` unread; the
// arguments are quoted words (remote_sh::quote_words), validated first by
// every caller: unit names, PIDs, firewall identifiers.

// sudo or dzdo, for a script that may give it a password, is never the one
// the login's PATH finds first: a program running as the login could put a
// `sudo` of its own there and catch the password. The scripts below that
// run the tool start with root_tool_find, run by /bin/sh:
//
// - PATH is set to ROOT_TOOL_PATH, and the tool (`$X`) is the first one
//   found there, by its full path.
// - Before a password goes anywhere, `safe` checks that the tool, the folder
//   it's in, and (through a link) the file it points to and its folder, are
//   owned by root and can't be changed by its group or by others. Otherwise
//   the script says UNSAFE and reads nothing: no password is sent.
//
// That's as far as a script can go: the login's own start-up files run
// before it (the login shell reads them), and could still catch what's
// typed. The password is only as safe as the server account.

/// Where the tool is looked for: never the login's own PATH.
pub(crate) const ROOT_TOOL_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin";

/// ROOT_TOOL_PATH, as the scripts get it (`SUB_PATH`). The tests' stand-ins
/// for sudo and dzdo are in the test server's working folder (`.`), first.
pub(crate) fn root_tool_path() -> String {
    if cfg!(test) {
        format!(".:{}", ROOT_TOOL_PATH)
    } else {
        ROOT_TOOL_PATH.to_string()
    }
}

/// Who besides root may own the tool (`SUB_UID`): no one in the app (0 is
/// root again); in the tests, whoever runs them, who owns the stand-ins.
pub(crate) fn root_tool_owner() -> String {
    #[cfg(all(test, unix))]
    {
        // SAFETY: geteuid can't fail.
        unsafe { libc::geteuid() }.to_string()
    }
    #[cfg(not(all(test, unix)))]
    {
        "0".to_string()
    }
}

/// The variables root_tool_find reads, for `tool`.
pub(crate) fn root_tool_env(tool: &str) -> Vec<(&'static str, String)> {
    vec![("SUB_TOOL", tool.to_string()), ("SUB_PATH", root_tool_path()), ("SUB_UID", root_tool_owner())]
}

/// The start of every script that runs sudo or dzdo (see above): `$X` is the
/// tool's full path (empty when there's none), and `safe` says whether it
/// may be given a password. POSIX sh. `unset -f` drops any function of
/// those names a shell brought from its environment.
macro_rules! root_tool_find {
    () => {
        r#"unset -f command ls readlink printf read echo pwd stty tr type 2>/dev/null
PATH=$SUB_PATH; export PATH
X=; o=$IFS; IFS=:
for d in $PATH; do case $d in /*) ;; *) d=$(pwd)/$d;; esac; if [ -f "$d/$SUB_TOOL" ] && [ -x "$d/$SUB_TOOL" ]; then X=$d/$SUB_TOOL; break; fi; done
IFS=$o
so() { set -f; set -- $(ls -ldnL "$1" 2>/dev/null); set +f; [ "$3" = 0 ] || [ "$3" = "$SUB_UID" ] || return 1; case $1 in ?????w*|????????w*) return 1;; esac; }
safe() { so "$X" && so "${X%/*}" || return 1; r=$(readlink -f "$X" 2>/dev/null); [ -z "$r" ] || { so "$r" && so "${r%/*}"; }; }"#
    };
}

/// What the scripts say, and the tool calls fail with, when the tool may not
/// be given a password (root_tool_find).
pub(crate) const SUDO_UNSAFE: &str = "[SUDO] FAILED: sudo/dzdo on this server isn't owned by root, so Submarine won't type your password into it.";

/// What the call fails with where the server's sh has no printf of its own:
/// the password would go through another program, on its command line,
/// where any program running as the login could read it.
pub(crate) const SUDO_NO_PRINTF: &str = "[SUDO] FAILED: this server's sh has no printf of its own, so Submarine won't pass your password through another program.";

/// root_probe's script: `$X -l <command>` (`-l` runs nothing, so all it
/// prints is the tool's own). With a password (`SUB_MODE=pw`), it says
/// `READ` once the tool is safe, and the password goes after that.
pub(crate) const ROOT_PROBE_SCRIPT: &str = concat!(
    root_tool_find!(),
    "\n",
    r#"if [ -z "$X" ]; then echo __SUB_SUDO:NO_SUDO; exit 0; fi
if [ "$SUB_MODE" = pw ]; then
if safe; then :; else echo __SUB_SUDO:UNSAFE; exit 0; fi
echo __SUB_SUDO:READ
OUT=$(LC_ALL=C LANG=C "$X" -S -k -p "" -l "$@" 2>&1)
elif [ -n "$SUB_NOTICKET" ]; then OUT=$(LC_ALL=C LANG=C "$X" -n -k -l "$@" 2>&1)
else OUT=$(LC_ALL=C LANG=C "$X" -n -l "$@" 2>&1); fi
if [ $? -eq 0 ]; then echo __SUB_SUDO:OK; else echo "__SUB_SUDO:FAIL:$(printf %s "$OUT" | tr "\n" " ")"; fi"#
);

/// The command that runs ROOT_PROBE_SCRIPT for `tool` and `args`; with
/// `no_tickets`, no sudo ticket counts (`-k`).
pub(crate) fn root_probe_command(tool: &str, password: bool, args: &[String], no_tickets: bool) -> String {
    let mut env = root_tool_env(tool);
    env.push(("SUB_MODE", if password { "pw" } else { "n" }.to_string()));
    if no_tickets {
        env.push(("SUB_NOTICKET", "1".into()));
    }
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    remote_sh::sh_command_encoded("/bin/sh", &env, ROOT_PROBE_SCRIPT, args)
}

/// Run a probe that may get the password: it goes only once the script has
/// said `__SUB_SUDO:READ` (its checks passed), then stdin ends. Returns what
/// it printed, up to 64 KB.
pub(crate) async fn exec_probe_with_password<H: russh::client::Handler>(session_arc: &HandleArc<H>, cmd: &str, password: &str, timeout_secs: u64) -> Result<String, String> {
    use russh::ChannelMsg;
    let mut channel = shell_sftp::open_session_channel(session_arc).await?;
    channel.exec(true, cmd.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut out: Vec<u8> = Vec::new();
    let work = async {
        let mut sent = false;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } | ChannelMsg::ExtendedData { ref data, .. } => {
                    if out.len() < 64 * 1024 {
                        out.extend_from_slice(data);
                    }
                    if !sent && String::from_utf8_lossy(&out).lines().any(|l| l.trim() == "__SUB_SUDO:READ") {
                        sent = true;
                        channel.data(&password_line(password)[..]).await.map_err(|e| e.to_string())?;
                        channel.eof().await.map_err(|e| e.to_string())?;
                    }
                }
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok::<_, String>(())
    };
    let done = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), work).await;
    let _ = channel.close().await;
    match done {
        Ok(r) => r?,
        Err(_) => return Err(format!("exec timed out after {}s", timeout_secs)),
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Can `tool` run `args` as root on this server? `tool -l <command>` asks
/// without running it, and also answers for sudoers rules scoped to one
/// command. `Err` is a [SUDO] code (see classify_sudo_failure).
pub(crate) async fn root_probe<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    tool: &str,
    password: Option<&str>,
    args: &[String],
    no_tickets: bool,
) -> Result<(), String> {
    let cmd = root_probe_command(tool, password.is_some(), args, no_tickets);
    let out = match password {
        Some(pw) => exec_probe_with_password(session_arc, &cmd, pw, 20).await?,
        None => exec_with_stdin_capture(session_arc, &cmd, None, 20).await?,
    };
    for line in out.lines() {
        match line.trim().strip_prefix("__SUB_SUDO:") {
            Some("OK") => return Ok(()),
            Some("NO_SUDO") => return Err(SUDO_NO_SUDO.into()),
            Some("UNSAFE") => return Err(SUDO_UNSAFE.into()),
            Some(rest) => {
                if let Some(msg) = rest.strip_prefix("FAIL:") {
                    return Err(classify_sudo_failure(msg));
                }
            }
            None => {}
        }
    }
    Err(format!("{}: unexpected reply from the server", SUDO_FAILED))
}

/// Root shells tried where sudo's or dzdo's rules allow only a shell — the
/// way many people get root in a terminal, `dzdo su -` — as the words after
/// the tool, in the order tried. Fixed text, never from input.
pub(crate) const ROOT_SHELL_FORMS: &[&str] = &["su -", "su - root", "su"];

/// What a root shell is given to run `script` (POSIX sh): root's own shell
/// may be csh or another, so it starts /bin/sh with the script in octal
/// (remote_sh), which they all pass on unread. One line.
pub(crate) fn root_shell_line(script: &str) -> String {
    format!("exec {}\n", remote_sh::sh_command_with("/bin/sh", &[], script, &[]))
}

/// Does `tool <form>` give a root shell without a password? The rules are
/// asked first (`-l`, which doesn't run anything), then the shell is started
/// and asked who it is.
pub(crate) async fn root_shell_probe<H: russh::client::Handler>(session_arc: &HandleArc<H>, tool: &'static str, form: &'static str, no_tickets: bool) -> Result<(), String> {
    let words: Vec<String> = form.split(' ').map(String::from).collect();
    root_probe(session_arc, tool, None, &words, no_tickets).await?;
    let line = root_shell_line("echo __SUB_SU_UID:$(id -u)");
    // In English (the C locale), so its refusals are understood.
    let k = if no_tickets { " -k" } else { "" };
    let out = exec_with_stdin_capture(session_arc, &format!("env LC_ALL=C LANG=C {} -n{} {}", tool, k, form), Some(line.as_bytes()), 20).await?;
    if out.lines().any(|l| l.trim() == "__SUB_SU_UID:0") {
        Ok(())
    } else if out.contains("__SUB_SU_UID:") {
        Err(SUDO_NOT_ALLOWED.into())
    } else {
        Err(classify_tool_output(&out))
    }
}

/// The first of `tools` that gives a root shell (ROOT_SHELL_FORMS) without
/// a password, and how. `tools` are the ones that refused the command itself
/// (NOT_ALLOWED: they have rules for the user, just not that one). A tool's
/// forms stop at the first other answer: when `-l su -` wants a password,
/// the others do too, and every refused ask is in the server's auth log.
pub(crate) async fn find_root_shell<H: russh::client::Handler>(session_arc: &HandleArc<H>, tools: &[&'static str], no_tickets: bool) -> Option<(&'static str, &'static str)> {
    let mut tried: Vec<String> = Vec::new();
    for &tool in tools {
        for &form in ROOT_SHELL_FORMS {
            match root_shell_probe(session_arc, tool, form, no_tickets).await {
                Ok(()) => return Some((tool, form)),
                Err(e) => {
                    let stop = e != SUDO_NOT_ALLOWED;
                    tried.push(format!("`{} {}`: {}", tool, form, e));
                    if stop {
                        break;
                    }
                }
            }
        }
    }
    if !tried.is_empty() {
        crate::applog::write_line("root", &format!("no root shell without a password ({})", tried.join("; ")));
    }
    None
}

/// The first of `tools` that gives a root shell (ROOT_SHELL_FORMS) with the
/// user's `password`: `dzdo su -` where the tool asks for it. `tools` are
/// the ones that took the password and refused the command itself. Each try
/// is one sign-in (ROOT_EXEC_SCRIPT checks the password on its own first),
/// and the forms are tried in order, stopping at the first that works; any
/// answer but "not allowed" (a wrong password above all) ends the search, so
/// a password the tool turns down is never tried again. `Ok(None)`: no form
/// is allowed.
pub(crate) async fn find_root_shell_with_password<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    tools: &[&'static str],
    password: &zeroize::Zeroizing<String>,
    no_tickets: bool,
) -> Result<Option<(&'static str, &'static str)>, String> {
    let mut tried: Vec<String> = Vec::new();
    for &tool in tools {
        for &form in ROOT_SHELL_FORMS {
            let mut access = ssh_manager::RootAccess::new(tool, Some(password.clone()), Some(form), None);
            access.no_tickets = no_tickets;
            match root_run(session_arc, &access, &["true".to_string()], 25, None).await {
                Ok(_) => {
                    crate::applog::write_line("root", &format!("as root through `{} {}`, with the password", tool, form));
                    return Ok(Some((tool, form)));
                }
                Err(e) if e == SUDO_NOT_ALLOWED => tried.push(format!("`{} {}`", tool, form)),
                Err(e) => {
                    crate::applog::write_line("root", &format!("`{} {}` with the password: {}", tool, form, e));
                    return Err(e);
                }
            }
        }
    }
    if !tried.is_empty() {
        crate::applog::write_line("root", &format!("no root shell with the password ({} not allowed)", tried.join(", ")));
    }
    Ok(None)
}

/// A new tag for one call's ROOT_EXEC_SCRIPT: 128 random bits, so nothing
/// the server or a command prints can pass for the script's own lines.
pub(crate) fn new_root_tag() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!("__SUB_{}", hex)
}

/// The password and a line break, in a buffer that never grows (so no copy
/// is left behind unzeroised).
pub(crate) fn password_line(password: &str) -> zeroize::Zeroizing<Vec<u8>> {
    let mut line = zeroize::Zeroizing::new(Vec::with_capacity(password.len() + 1));
    line.extend_from_slice(password.as_bytes());
    line.push(b'\n');
    line
}

/// How every command runs as root (POSIX sh, remote_sh::sh_command, the
/// command's words as "$@"). With a password (`SUB_PW`), the app writes it
/// as one line once the script says `READ`, and:
///
/// 1. it goes to `$SUB_TOOL -S -v`, on a pipe that holds nothing else: the
///    tool checks it and, when it's wrong, asks again and reads the end of
///    the pipe. One sign-in, one failed attempt at most, and nothing the app
///    sends later can ever be taken for a second try. Whatever prompt the
///    tool or PAM would show doesn't matter (`-p ""`, no prompt is looked
///    for). The answer comes back as `V:<code>`.
/// 2. When the tool now has a ticket for this shell (`-n -v` works: sudo's,
///    sudo-rs's and dzdo's default), the command runs with `-n`, so it is
///    never given the password, even where its rule needs none.
/// 3. Where the tool keeps no ticket (`timestamp_timeout=0`), the tool is
///    run asking with `<tag>:PW:` as its prompt, and the app answers only
///    that prompt.
///
/// `RUN:t`, `RUN:p` or (no password) `RUN:n` says the command starts; all
/// it prints comes after. In a terminal (`SUB_TTY`) echo is off while the
/// password is read. The command isn't exec'd: a ticket kept for the
/// shell's children (no terminal: sudo's per-parent ticket) must still be
/// its own.
///
/// The tool is found as root_tool_find says, and is given a password only
/// once `safe` says it may (else `UNSAFE`); with no tool at all, `NO_TOOL`.
/// The password goes from `read` to the tool through printf only where it's
/// the shell's own (no other program, whose command line others could
/// read, ever holds it; a here-document could write it to a file): else
/// `NO_PRINTF`.
///
/// With `SUB_NOTICKET` (the AI's root, #120) no sudo ticket is used or left
/// behind: every call has `-k`, so a sign-in of yours (with a ticket shared
/// across terminals, `timestamp_type=global`) never counts, and the AI's own
/// check of the password leaves none. The command then runs with `-n -k`
/// where the rules allow it without a password (it never gets the password),
/// else with the tool asking again (`RUN:p`, its question answered once).
pub(crate) const ROOT_EXEC_SCRIPT: &str = concat!(
    root_tool_find!(),
    "\n",
    r#"T=$SUB_TAG
unset SUB_TAG
if [ -n "$SUB_ERR" ]; then exec 2>&1; fi
if [ -z "$X" ]; then echo "$T:NO_TOOL"; exit 95; fi
K=
if [ -n "$SUB_NOTICKET" ]; then K=-k; fi
if [ -z "$SUB_PW" ]; then echo "$T:RUN:n"; "$X" -n $K "$@"; exit $?; fi
if safe; then :; else echo "$T:UNSAFE"; exit 96; fi
case $(LC_ALL=C type printf 2>/dev/null) in *builtin*) ;; *) echo "$T:NO_PRINTF"; exit 96;; esac
if [ -n "$SUB_TTY" ]; then stty -echo 2>/dev/null; fi
echo "$T:READ"
IFS= read -r p || exit 97
if [ -n "$SUB_TTY" ]; then stty echo 2>/dev/null; fi
printf '%s\n' "$p" | LC_ALL=C LANG=C "$X" -S $K -p "" -v 2>&1
r=$?
p=
echo "$T:V:$r"
if [ "$r" != 0 ]; then exit 98; fi
if [ -z "$K" ] && "$X" -n -v >/dev/null 2>&1; then echo "$T:RUN:t"; "$X" -n "$@"; exit $?; fi
if [ -n "$K" ] && LC_ALL=C LANG=C "$X" -n -k -l "$@" >/dev/null 2>&1; then echo "$T:RUN:n"; "$X" -n -k "$@"; exit $?; fi
echo "$T:RUN:p"
if [ -n "$SUB_TTY" ]; then "$X" $K -p "$T:PW:" "$@"; else "$X" -S $K -p "$T:PW:" "$@"; fi
exit $?"#
);

/// The command that runs `words` as root with `tool` (ROOT_EXEC_SCRIPT),
/// under /bin/sh. The words go as octal (remote_sh::sh_command_encoded), so
/// a command with `!` or a line break reaches it from a csh or tcsh login.
pub(crate) fn root_exec_command(tool: &str, tag: &str, password: bool, pty: bool, stderr_to_stdout: bool, no_tickets: bool, words: &[String]) -> String {
    let mut env = root_tool_env(tool);
    if no_tickets {
        env.push(("SUB_NOTICKET", "1".into()));
    }
    env.push(("SUB_TAG", tag.to_string()));
    if password {
        env.push(("SUB_PW", "1".into()));
    }
    if pty {
        env.push(("SUB_TTY", "1".into()));
    }
    if stderr_to_stdout {
        env.push(("SUB_ERR", "1".into()));
    }
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    remote_sh::sh_command_encoded("/bin/sh", &env, ROOT_EXEC_SCRIPT, words)
}

/// What root_feed gives a channel running ROOT_EXEC_SCRIPT.
pub(crate) struct RootFeed<'a> {
    /// The script's tag (new_root_tag).
    pub(crate) tag: &'a str,
    pub(crate) password: Option<&'a str>,
    /// What goes on stdin once the command starts: a root shell's line.
    pub(crate) then: &'a [u8],
    /// The command reads no more: stdin is closed after `then`.
    pub(crate) end: bool,
    /// A terminal: the tool asks on it (ROOT_EXEC_SCRIPT's third case).
    pub(crate) pty: bool,
    /// Set when the tool's check of the password failed (`V:` not 0),
    /// whatever it said: the AI forgets a kept password then, even when the
    /// tool's words aren't English (ai/exec.rs).
    pub(crate) turned_down: Option<&'a std::sync::atomic::AtomicBool>,
}

/// One call's turn with the tab's kept password, settled as soon as the
/// tool has answered it (not when the command ends), so the tab's other
/// calls wait only that long.
pub(crate) struct TurnHook<'a> {
    pub(crate) turn: PasswordTurn,
    pub(crate) state: &'a SshState,
    pub(crate) session_id: &'a str,
    pub(crate) access: &'a ssh_manager::RootAccess,
}

impl TurnHook<'_> {
    async fn settle(self, worked: Option<bool>) {
        self.turn.finish(self.state, self.session_id, self.access, worked).await;
    }
}

/// How long the tool may take to check a password: sssd or AD can be slow,
/// and nothing else is sent meanwhile.
pub(crate) const ROOT_CHECK_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Drive ROOT_EXEC_SCRIPT on `channel`: the password when it asks, the
/// tool's answer, then `then`. Returns what the command printed so far.
pub(crate) async fn root_feed(channel: &mut russh::Channel<russh::client::Msg>, feed: RootFeed<'_>, hook: Option<TurnHook<'_>>) -> Result<Vec<u8>, String> {
    use russh::ChannelMsg;
    use std::time::{Duration, Instant};
    // Where `what` is in `seen` when it's the first thing there, blank space
    // aside; None while it isn't (anything else first, and it never is).
    let first = |seen: &[u8], what: &[u8]| {
        let lead = seen.iter().take_while(|b| b.is_ascii_whitespace()).count();
        seen[lead..].starts_with(what).then_some(lead)
    };
    let read_tag = format!("{}:READ", feed.tag);
    let verdict_tag = format!("{}:V:", feed.tag);
    let run_tag = format!("{}:RUN:", feed.tag);
    let unsafe_tag = format!("{}:UNSAFE", feed.tag);
    let no_printf_tag = format!("{}:NO_PRINTF", feed.tag);
    let no_tool_tag = format!("{}:NO_TOOL", feed.tag);
    let mut hook = hook;
    // The script's lines up to RUN; what the tool said since READ.
    let mut pending: Vec<u8> = Vec::new();
    let mut said: Vec<u8> = Vec::new();
    let mut deadline = Instant::now() + Duration::from_secs(20);
    let (how, mut rest) = 'lines: loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let data = match tokio::time::timeout(left, channel.wait()).await {
            Ok(Some(ChannelMsg::Data { data })) | Ok(Some(ChannelMsg::ExtendedData { data, .. })) => data,
            Ok(Some(ChannelMsg::Close | ChannelMsg::Eof | ChannelMsg::ExitStatus { .. })) | Ok(None) => {
                return Err(tool_answer(&String::from_utf8_lossy(&said)).unwrap_or_else(|| format!("{}: it ended before the command started", SUDO_FAILED)));
            }
            Ok(Some(_)) => continue,
            Err(_) => return Err(format!("{}: the server didn't answer", SUDO_FAILED)),
        };
        pending.extend_from_slice(&data);
        while let Some(nl) = pending.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = pending.drain(..=nl).collect();
            let text = String::from_utf8_lossy(&line_bytes);
            // The script's own lines may follow a prompt the tool or PAM
            // printed without a line break: the tag is looked for anywhere.
            let line = match text.find(feed.tag) {
                Some(at) => {
                    if at > 0 && said.len() < 8192 {
                        said.extend_from_slice(text[..at].as_bytes());
                        said.push(b'\n');
                    }
                    text[at..].trim()
                }
                None => text.trim(),
            };
            if line == unsafe_tag {
                return Err(SUDO_UNSAFE.into());
            } else if line == no_printf_tag {
                return Err(SUDO_NO_PRINTF.into());
            } else if line == no_tool_tag {
                return Err(SUDO_NO_SUDO.into());
            } else if line == read_tag {
                let Some(pw) = feed.password else { return Err(format!("{}: unexpected reply from the server", SUDO_FAILED)) };
                channel.data(&password_line(pw)[..]).await.map_err(|e| e.to_string())?;
                deadline = Instant::now() + ROOT_CHECK_WAIT;
            } else if let Some(code) = line.strip_prefix(verdict_tag.as_str()) {
                if code == "0" {
                    if let Some(h) = hook.take() {
                        h.settle(Some(true)).await;
                    }
                    deadline = Instant::now() + Duration::from_secs(20);
                } else {
                    if let Some(t) = feed.turned_down {
                        t.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    let answer = tool_answer(&String::from_utf8_lossy(&said));
                    let wrong = answer.as_deref() == Some(SUDO_WRONG_PASSWORD);
                    if let Some(h) = hook.take() {
                        h.settle(wrong.then_some(false)).await;
                    }
                    return Err(answer.unwrap_or_else(|| {
                        let text: String = String::from_utf8_lossy(&said).trim().chars().take(300).collect();
                        format!("{}: the password couldn't be checked{}{}", SUDO_FAILED, if text.is_empty() { "" } else { ": " }, text)
                    }));
                }
            } else if let Some(how) = line.strip_prefix(run_tag.as_str()) {
                break 'lines (how.to_string(), std::mem::take(&mut pending));
            } else if said.len() < 8192 {
                said.extend_from_slice(&line_bytes);
            }
        }
    };
    if how == "p" {
        // No ticket: the tool asks again, with the tag as its prompt, and
        // only that prompt is answered, and only when it's the first thing
        // after RUN:p: anything before it is something else talking (a
        // command that runs without asking, printing what looks like the
        // prompt), and the password isn't sent. A prompt of PAM's own instead
        // isn't answered: in a terminal with nothing else to send, the user
        // sees it and types; otherwise it fails.
        let pw = feed.password.ok_or_else(|| format!("{}: unexpected reply from the server", SUDO_FAILED))?;
        let prompt = format!("{}:PW:", feed.tag);
        let start = Instant::now();
        let mut quiet_by: Option<Instant> = None;
        let mut asked = first(&rest, prompt.as_bytes());
        while asked.is_none() {
            let until = quiet_by.unwrap_or(start + Duration::from_secs(20));
            let left = until.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, channel.wait()).await {
                Ok(Some(ChannelMsg::Data { data })) | Ok(Some(ChannelMsg::ExtendedData { data, .. })) => {
                    rest.extend_from_slice(&data);
                    asked = first(&rest, prompt.as_bytes());
                    if !String::from_utf8_lossy(&rest).trim().is_empty() {
                        quiet_by = Some(Instant::now() + Duration::from_millis(500));
                    }
                }
                Ok(Some(ChannelMsg::Close | ChannelMsg::Eof)) | Ok(None) => {
                    let text = String::from_utf8_lossy(&rest);
                    return match tool_answer(&text) {
                        Some(answer) => Err(answer),
                        // It ran without asking (a NOPASSWD rule) and is done.
                        None if feed.then.is_empty() => Ok(rest),
                        None => Err(format!("{}: it ended before the command started", SUDO_FAILED)),
                    };
                }
                Ok(Some(_)) => {}
                Err(_) => {
                    let text = String::from_utf8_lossy(&rest);
                    // The command's own output, ending its lines, and the tool
                    // never asked: a NOPASSWD rule. The password isn't given.
                    let ran = text.trim_end_matches([' ', '\t']).ends_with('\n') && tool_answer(&text).is_none();
                    if feed.then.is_empty() && (ran || feed.pty) {
                        if feed.end {
                            channel.eof().await.map_err(|e| e.to_string())?;
                        }
                        return Ok(rest);
                    }
                    return Err(if text.trim().is_empty() {
                        format!("{}: the server didn't answer", SUDO_FAILED)
                    } else {
                        format!(
                            "{}: something came before sudo's or dzdo's own question for the password (a warning, or a prompt of its own), so the app didn't send it",
                            SUDO_FAILED
                        )
                    });
                }
            }
        }
        let at = asked.unwrap_or(0);
        rest.drain(..at + prompt.len());
        channel.data(&password_line(pw)[..]).await.map_err(|e| e.to_string())?;
        if feed.pty && !feed.then.is_empty() {
            // The tool is done with the terminal (and drops what was typed
            // ahead) once the root shell shows its prompt.
            let start = Instant::now();
            while String::from_utf8_lossy(&rest).trim().is_empty() {
                let left = (start + Duration::from_secs(20)).saturating_duration_since(Instant::now());
                match tokio::time::timeout(left, channel.wait()).await {
                    Ok(Some(ChannelMsg::Data { data })) | Ok(Some(ChannelMsg::ExtendedData { data, .. })) => rest.extend_from_slice(&data),
                    Ok(Some(ChannelMsg::Close | ChannelMsg::Eof)) | Ok(None) => {
                        return Err(classify_tool_output(&String::from_utf8_lossy(&rest)));
                    }
                    Ok(Some(_)) => {}
                    Err(_) => return Err(format!("{}: the root shell didn't start", SUDO_FAILED)),
                }
            }
        }
    }
    if !feed.then.is_empty() {
        channel.data(feed.then).await.map_err(|e| e.to_string())?;
    }
    if feed.end {
        channel.eof().await.map_err(|e| e.to_string())?;
    }
    Ok(rest)
}

/// Run ROOT_EXEC_SCRIPT's `cmd`, feed it (root_feed), and collect what the
/// command prints until it ends: up to `max` bytes, within `timeout_secs`.
/// Returns the command's exit code too.
pub(crate) async fn exec_fed_capture<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    cmd: &str,
    feed: RootFeed<'_>,
    hook: Option<TurnHook<'_>>,
    timeout_secs: u64,
    max: usize,
) -> Result<(i32, String), String> {
    use russh::ChannelMsg;
    let mut channel = shell_sftp::open_session_channel(session_arc).await?;
    channel.exec(true, cmd.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut code: Option<u32> = None;
    let work = async {
        let mut out = root_feed(&mut channel, feed, hook).await?;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } | ChannelMsg::ExtendedData { ref data, .. } => {
                    if out.len() < max {
                        out.extend_from_slice(data);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok::<_, String>(out)
    };
    let out = match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), work).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            let _ = channel.close().await;
            return Err(e);
        }
        Err(_) => {
            let _ = channel.close().await;
            return Err(format!("{} after {}s", ROOT_TIMED_OUT, timeout_secs));
        }
    };
    Ok((code.map(|c| c as i32).unwrap_or(-1), String::from_utf8_lossy(&out).into_owned()))
}

/// How a root command that ran out of time fails: it may have done its work
/// (a restart that took long), so it isn't run again.
pub(crate) const ROOT_TIMED_OUT: &str = "the command timed out";

/// Run `args` as root with `access`: the command's exit code and output.
/// `hook`: the tab's turn with a kept password, settled once the tool has
/// answered it.
pub(crate) async fn root_run<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    access: &ssh_manager::RootAccess,
    args: &[String],
    timeout_secs: u64,
    hook: Option<TurnHook<'_>>,
) -> Result<(i32, String), String> {
    let tool = if ELEVATION_TOOLS.contains(&access.tool) { access.tool } else { "sudo" };
    let tag = new_root_tag();
    let password = access.password.as_ref().map(|p| p.as_str());
    if let Some(form) = access.shell.filter(|s| ROOT_SHELL_FORMS.contains(s)) {
        // The root shell's own start-up (a profile that prints) comes before
        // the start marker and isn't part of the output. The marker says who
        // the shell is, so a kept root shell needs no check of its own before
        // each command (run_as_root).
        let line = root_shell_line(&format!(
            "echo __SUB_ROOT_OUT__:$(id -u); {} 2>&1; echo __SUB_EXITCODE:$?",
            remote_sh::quote_words(args)
        ));
        let words: Vec<String> = form.split(' ').map(String::from).collect();
        let cmd = root_exec_command(tool, &tag, password.is_some(), false, true, access.no_tickets, &words);
        let feed = RootFeed { tag: &tag, password, then: line.as_bytes(), end: true, pty: false, turned_down: None };
        let (_, out) = exec_fed_capture(session_arc, &cmd, feed, hook, timeout_secs, ROOT_OUTPUT_MAX).await?;
        let Some(start) = out.find("__SUB_ROOT_OUT__:") else {
            return Err(classify_tool_output(&out));
        };
        let rest = &out[start + "__SUB_ROOT_OUT__:".len()..];
        let (uid, after) = rest.split_once('\n').unwrap_or((rest, ""));
        if uid.trim() != "0" {
            return Err(SUDO_NOT_ALLOWED.into());
        }
        return Ok(parse_exit_marker(after));
    }
    let cmd = root_exec_command(tool, &tag, password.is_some(), false, true, access.no_tickets, args);
    let feed = RootFeed { tag: &tag, password, then: b"", end: true, pty: false, turned_down: None };
    let (code, out) = exec_fed_capture(session_arc, &cmd, feed, hook, timeout_secs, ROOT_OUTPUT_MAX).await?;
    Ok((code, out.trim_end_matches('\n').to_string()))
}

/// What root_run keeps of a command's output: a container's logs or a
/// firewall's rules can run long.
pub(crate) const ROOT_OUTPUT_MAX: usize = 4 * 1024 * 1024;

/// What a root channel's output starts after when it runs through a root
/// shell (root_channel): the shell's own start-up comes before it. Followed
/// by the shell's uid and a line break.
pub(crate) const ROOT_STREAM_MARKER: &str = "__SUB_ROOT_STREAM__:";

/// A channel running `args` as root on this tab's server for as long as the
/// command runs: a container's live logs, or (with `pty`, its size) a
/// terminal in a container. Root is found as for run_as_root: what the tab
/// has, else a look, with `password` when the tool asks for one. Returns
/// the channel; through a root shell without a terminal, the marker its
/// output starts after (ROOT_STREAM_MARKER); and what the command already
/// wrote as it started.
pub(crate) async fn root_channel(
    state: &SshState,
    session_id: &str,
    args: &[String],
    password: Option<String>,
    pty: Option<(u32, u32)>,
) -> Result<(russh::Channel<russh::client::Msg>, Option<&'static str>, Vec<u8>), String> {
    let session_arc = {
        let connections = state.connections.lock().await;
        connections.get(session_id).map(Arc::clone).ok_or_else(|| "Session not connected".to_string())?
    };
    let typed = password.is_some();
    let access = root_access_for(state, session_id, &session_arc, args, password, true).await?;
    let turn = password_turn(state, session_id, &access, typed).await?;
    let hook = TurnHook { turn, state, session_id, access: &access };
    root_channel_open(&session_arc, &access, args, pty, Some(hook)).await
}

pub(crate) async fn root_channel_open<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    access: &ssh_manager::RootAccess,
    args: &[String],
    pty: Option<(u32, u32)>,
    hook: Option<TurnHook<'_>>,
) -> Result<(russh::Channel<russh::client::Msg>, Option<&'static str>, Vec<u8>), String> {
    let mut channel = shell_sftp::open_session_channel(session_arc).await?;
    if let Some((cols, rows)) = pty {
        channel
            .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| e.to_string())?;
    }
    // From here a failure closes the channel: a dropped one stays open on the
    // server, with whatever the tool started in it.
    match root_channel_start(&mut channel, access, args, pty.is_some(), hook, None).await {
        Ok((marker, early)) => Ok((channel, marker, early)),
        Err(e) => {
            let _ = channel.close().await;
            Err(e)
        }
    }
}

/// Start `args` as root with `access` on `channel`, open and with nothing
/// run on it yet (with a terminal when `pty`): ROOT_EXEC_SCRIPT, fed by
/// root_feed. Returns, through a root shell without a terminal, the marker
/// the command's output starts after (ROOT_STREAM_MARKER), and what the
/// command already wrote as it started. On a failure the caller closes the
/// channel. The AI's root commands start this way too (ai/exec.rs).
/// `turned_down` is set when the tool's check of the password failed.
pub(crate) async fn root_channel_start(
    channel: &mut russh::Channel<russh::client::Msg>,
    access: &ssh_manager::RootAccess,
    args: &[String],
    pty: bool,
    hook: Option<TurnHook<'_>>,
    turned_down: Option<&std::sync::atomic::AtomicBool>,
) -> Result<(Option<&'static str>, Vec<u8>), String> {
    let tool = if ELEVATION_TOOLS.contains(&access.tool) { access.tool } else { "sudo" };
    let tag = new_root_tag();
    let password = access.password.as_ref().map(|p| p.as_str());
    let (words, line, marker) = match access.shell.filter(|s| ROOT_SHELL_FORMS.contains(s)) {
        Some(form) => {
            let script = if pty {
                // In a terminal the line given to the root shell shows as it's
                // typed; the screen is cleared before the command starts.
                format!("printf '\\033[H\\033[2J\\033[3J'; exec {}", remote_sh::quote_words(args))
            } else {
                format!("echo {}$(id -u); exec {} 2>&1", ROOT_STREAM_MARKER, remote_sh::quote_words(args))
            };
            let words: Vec<String> = form.split(' ').map(String::from).collect();
            (words, root_shell_line(&script), (!pty).then_some(ROOT_STREAM_MARKER))
        }
        None => (args.to_vec(), String::new(), None),
    };
    // Without a terminal, what the tool says when it refuses comes on
    // stderr, which a stream doesn't read: it goes to stdout.
    let command = root_exec_command(tool, &tag, password.is_some(), pty, !pty, access.no_tickets, &words);
    channel.exec(true, command.as_bytes()).await.map_err(|e| e.to_string())?;
    let feed = RootFeed { tag: &tag, password, then: line.as_bytes(), end: !pty, pty, turned_down };
    let early = root_feed(channel, feed, hook).await?;
    Ok((marker, early))
}

/// How long a kept password that worked is taken to work still: calls with
/// it go at once and side by side.
pub(crate) const PASSWORD_TRUSTED_FOR: std::time::Duration = std::time::Duration::from_secs(300);

/// One call's turn with the tab's kept password (RootAccess::checked).
pub(crate) struct PasswordTurn {
    /// Held while a password not known to work lately is tried: the tab's
    /// other calls wait for its answer rather than each sign in with it.
    pub(crate) held: Option<tokio::sync::OwnedMutexGuard<Option<std::time::Instant>>>,
}

/// Wait for this call's turn with `access`'s password. `Err` when it was
/// forgotten meanwhile because it stopped working: the tab asks for it.
pub(crate) async fn password_turn(state: &SshState, session_id: &str, access: &ssh_manager::RootAccess, typed: bool) -> Result<PasswordTurn, String> {
    if access.password.is_none() {
        return Ok(PasswordTurn { held: None });
    }
    let guard = Arc::clone(&access.checked).lock_owned().await;
    if !typed && !root_access_is_kept(state, session_id, access).await {
        return Err(SUDO_WRONG_PASSWORD.into());
    }
    let trusted = guard.is_some_and(|t| t.elapsed() < PASSWORD_TRUSTED_FOR);
    Ok(PasswordTurn { held: (!trusted).then_some(guard) })
}

impl PasswordTurn {
    /// What the tool said of the password: `Some(true)` it worked,
    /// `Some(false)` it was turned down (forgotten for the whole tab at
    /// once), `None` no answer either way.
    async fn finish(self, state: &SshState, session_id: &str, access: &ssh_manager::RootAccess, worked: Option<bool>) {
        let Some(pw) = &access.password else { return };
        let Some(worked) = worked else { return };
        if !worked {
            forget_root_password(state, session_id, pw).await;
        }
        let when = worked.then(std::time::Instant::now);
        match self.held {
            Some(mut held) => *held = when,
            // Another call is trying it: its answer counts.
            None => {
                if let Ok(mut checked) = access.checked.try_lock() {
                    *checked = when;
                }
            }
        }
    }
}

/// Is `access` still the one the tab keeps?
pub(crate) async fn root_access_is_kept(state: &SshState, session_id: &str, access: &ssh_manager::RootAccess) -> bool {
    state.root_access.lock().await.get(session_id).is_some_and(|a| Arc::ptr_eq(&a.checked, &access.checked))
}

/// What the tab already has for root: its own, else its SFTP root mode's
/// (kept as its own from then on, so the tab's calls share one answer on
/// whether its password still works). Not a password that stopped working.
pub(crate) async fn known_root_access(state: &SshState, session_id: &str) -> Option<ssh_manager::RootAccess> {
    // Lock order: root_access, then sftp_elevation.
    let mut own = state.root_access.lock().await;
    if let Some(a) = own.get(session_id) {
        return Some(a.clone());
    }
    let e = state.sftp_elevation.lock().await.get(session_id).cloned()?;
    if e.password_failed {
        return None;
    }
    let a = ssh_manager::RootAccess::new(e.tool, e.password.clone(), e.shell, None);
    own.insert(session_id.to_string(), a.clone());
    Some(a)
}

/// Forget `access` for the tab, if it's still the one kept (another call
/// may have put a newer one there).
pub(crate) async fn forget_root_access(state: &SshState, session_id: &str, access: &ssh_manager::RootAccess) {
    let mut own = state.root_access.lock().await;
    if own.get(session_id).is_some_and(|a| Arc::ptr_eq(&a.checked, &access.checked)) {
        own.remove(session_id);
    }
}

/// The tool turned down the tab's kept password `pw`: forget it everywhere
/// the tab keeps it at once (its root access and its SFTP root mode), so
/// nothing tries it again; the next action asks for the password.
pub(crate) async fn forget_root_password(state: &SshState, session_id: &str, pw: &str) {
    crate::applog::write_line("root", "the kept password was turned down; forgotten for this tab");
    {
        let mut own = state.root_access.lock().await;
        if own.get(session_id).and_then(|a| a.password.as_ref()).is_some_and(|p| p.as_str() == pw) {
            own.remove(session_id);
        }
    }
    let mut modes = state.sftp_elevation.lock().await;
    if let Some(e) = modes.get_mut(session_id) {
        if e.password.as_ref().is_some_and(|p| p.as_str() == pw) {
            e.password_failed = true;
        }
    }
}

/// A password the user just typed worked for `access.tool`: where the tab's
/// SFTP root mode had that tool's password turned down, it takes this one.
pub(crate) async fn revive_sftp_password(state: &SshState, session_id: &str, access: &ssh_manager::RootAccess) {
    let Some(pw) = &access.password else { return };
    let mut modes = state.sftp_elevation.lock().await;
    if let Some(e) = modes.get_mut(session_id) {
        if e.password_failed && e.tool == access.tool {
            e.password = Some(pw.clone());
            e.password_failed = false;
        }
    }
}

/// How this tab gets root for `args`: what it already has (its own, else its
/// SFTP root mode's) when that still works, else the first tool that lets it
/// — without a password, then with `password`, the command itself and then a
/// root shell (`dzdo su -`). Kept for the tab. `Err` is a [SUDO] code; with
/// NEED_PASSWORD or WRONG_PASSWORD the panel asks for the password and calls
/// again with it.
pub(crate) async fn root_access_for(
    state: &SshState,
    session_id: &str,
    session_arc: &SessionHandleArc,
    args: &[String],
    password: Option<String>,
    use_known: bool,
) -> Result<ssh_manager::RootAccess, String> {
    let password = password.map(zeroize::Zeroizing::new).filter(|p| !p.is_empty());
    if let Some(p) = &password {
        if p.contains(['\n', '\r', '\0']) {
            return Err(format!("{}: the password contains a line break", SUDO_FAILED));
        }
    }
    // A typed password means what the tab had stopped working: start over.
    if let Some(pw) = password {
        return typed_root_access(state, session_id, session_arc, args, pw).await;
    }
    if use_known {
        if let Some(a) = known_root_access(state, session_id).await {
            // A root shell runs anything, and root_run checks it's still root
            // as it runs the command (run_as_root looks again if not). With a
            // password, the run itself is the check: asking first (`-l`)
            // would be a second sign-in per command. Without one, the tool's
            // rules are asked about this command (no sign-in).
            if a.shell.is_some() || a.password.is_some() {
                return Ok(a);
            }
            if root_probe(session_arc, a.tool, None, args, false).await.is_ok() {
                return Ok(a);
            }
            // This tool doesn't allow this command (a sudoers rule for
            // sftp-server only, say): look again.
            forget_root_access(state, session_id, &a).await;
        }
    }
    let a = find_root_without_password(session_arc, args, false).await.map_err(|(e, _)| e)?;
    state.root_access.lock().await.insert(session_id.to_string(), a.clone());
    Ok(a)
}

/// The first tool (ELEVATION_TOOLS) whose rules let it run `args` as root
/// without a password, else, as SFTP root mode does (probe_sudo_sftp), a
/// root shell without one (find_root_shell). Signs in nowhere: the rules are
/// asked with `-n -l`, which runs nothing. `Err`: the [SUDO] code worth
/// showing, and the tool that gave it (None for NO_SUDO). With
/// `no_tickets`, a sudo ticket doesn't count (`-k`), and what's found uses
/// none (RootAccess::no_tickets).
pub(crate) async fn find_root_without_password<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    args: &[String],
    no_tickets: bool,
) -> Result<ssh_manager::RootAccess, (String, Option<&'static str>)> {
    use ssh_manager::RootAccess;
    let mut best: Option<(String, &'static str)> = None;
    let mut refused: Vec<&'static str> = Vec::new();
    for &tool in ELEVATION_TOOLS {
        match root_probe(session_arc, tool, None, args, no_tickets).await {
            Ok(()) => return Ok(RootAccess { no_tickets, ..RootAccess::new(tool, None, None, None) }),
            Err(e) => {
                if e == SUDO_NOT_ALLOWED {
                    refused.push(tool);
                }
                if e != SUDO_NO_SUDO {
                    crate::applog::write_line("root", &format!("{} may not run {} itself ({})", tool, args.first().map(String::as_str).unwrap_or(""), e));
                }
                if best.as_ref().is_none_or(|(b, _)| elevation_error_rank(&e) > elevation_error_rank(b)) {
                    best = Some((e, tool));
                }
            }
        }
    }
    if let Some((tool, form)) = find_root_shell(session_arc, &refused, no_tickets).await {
        crate::applog::write_line("root", &format!("as root through `{} {}`, no password", tool, form));
        return Ok(RootAccess { no_tickets, ..RootAccess::new(tool, None, Some(form), None) });
    }
    Err(match best {
        Some((e, _)) if e == SUDO_NO_SUDO => (e, None),
        Some((e, tool)) => (e, Some(tool)),
        None => (SUDO_NO_SUDO.into(), None),
    })
}

/// The tab's gate for passwords the user types (ssh_manager::RootGate).
pub(crate) async fn root_gate(state: &SshState, session_id: &str) -> Arc<tokio::sync::Mutex<ssh_manager::RootGate>> {
    Arc::clone(state.root_gates.lock().await.entry(session_id.to_string()).or_default())
}

/// A digest of `password` with this run's own random salt, to tell a
/// password just turned down when it's typed again (RootGate::rejected).
pub(crate) fn password_digest(password: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    static SALT: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    let salt = SALT.get_or_init(|| {
        let mut b = [0u8; 32];
        rand::rng().fill_bytes(&mut b);
        b
    });
    let mut h = Sha256::new();
    h.update(b"submarine-root-password\0");
    h.update(salt);
    h.update(password.as_bytes());
    h.finalize().into()
}

/// How long a password sudo or dzdo turned down is answered "wrong" again
/// at once, without trying it.
pub(crate) const REJECTED_FOR: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether `password` was turned down for this tab a moment ago.
pub(crate) fn just_rejected(gate: &ssh_manager::RootGate, password: &str) -> bool {
    gate.rejected.is_some_and(|(d, at)| d == password_digest(password) && at.elapsed() < REJECTED_FOR)
}

/// root_access_for with a password the user typed. Every call of the tab
/// that was waiting for the password gets the same answer (rootInvoke), so
/// they take turns: the first tries it; the others take what it found, or
/// are told it's wrong without trying it again (an account lockout after a
/// few failed attempts). Then, as SFTP root mode: a tool that isn't there or
/// doesn't allow the user hands over to the next; a wrong password ends it
/// (one failed attempt, not one per tool); a tool that took the password but
/// allows only a root shell gets `dzdo su -`.
pub(crate) async fn typed_root_access<H: russh::client::Handler>(
    state: &SshState,
    session_id: &str,
    session_arc: &HandleArc<H>,
    args: &[String],
    pw: zeroize::Zeroizing<String>,
) -> Result<ssh_manager::RootAccess, String> {
    use ssh_manager::RootAccess;
    let gate = root_gate(state, session_id).await;
    let mut gate = gate.lock().await;
    if just_rejected(&gate, &pw) {
        return Err(SUDO_WRONG_PASSWORD.into());
    }
    let kept = state.root_access.lock().await.get(session_id).cloned();
    if let Some(a) = kept.filter(|a| a.password.as_ref().is_some_and(|p| p.as_str() == pw.as_str())) {
        return Ok(a);
    }
    let found = find_root_with_password(session_arc, args, &pw, false).await;
    match found {
        Ok((tool, shell)) => {
            gate.rejected = None;
            let a = RootAccess::new(tool, Some(pw), shell, Some(std::time::Instant::now()));
            state.root_access.lock().await.insert(session_id.to_string(), a.clone());
            revive_sftp_password(state, session_id, &a).await;
            Ok(a)
        }
        Err(e) => {
            if e == SUDO_WRONG_PASSWORD {
                gate.rejected = Some((password_digest(&pw), std::time::Instant::now()));
            }
            Err(e)
        }
    }
}

/// How `args` run as root with the user's typed `pw`: the first tool that
/// takes it and allows them, else a root shell through one that takes it but
/// allows only that (`dzdo su -`, find_root_shell_with_password). A tool that
/// isn't there or doesn't allow the user hands over to the next, and any
/// other answer (a wrong password above all) ends it: a mistyped password is
/// one failed attempt. Returns the tool and the root shell's words, if any.
pub(crate) async fn find_root_with_password<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    args: &[String],
    pw: &zeroize::Zeroizing<String>,
    no_tickets: bool,
) -> Result<(&'static str, Option<&'static str>), String> {
    let mut refused: Vec<&'static str> = Vec::new();
    for tool in ELEVATION_TOOLS {
        match root_probe(session_arc, tool, Some(pw.as_str()), args, no_tickets).await {
            Ok(()) => return Ok((*tool, None)),
            Err(e) if e == SUDO_NO_SUDO => continue,
            Err(e) if e == SUDO_NOT_ALLOWED => refused.push(*tool),
            Err(e) => return Err(e),
        }
    }
    match find_root_shell_with_password(session_arc, &refused, pw, no_tickets).await? {
        Some((tool, form)) => Ok((tool, Some(form))),
        None => Err(SUDO_NOT_ALLOWED.to_string()),
    }
}

/// Run `args` as root on this tab's server: exit code, output, and the tool.
pub(crate) async fn run_as_root(
    state: &SshState,
    session_id: &str,
    args: &[String],
    password: Option<String>,
    timeout_secs: u64,
) -> Result<(i32, String, &'static str), String> {
    let session_arc = {
        let connections = state.connections.lock().await;
        connections.get(session_id).map(Arc::clone).ok_or_else(|| "Session not connected".to_string())?
    };
    let typed = password.is_some();
    let access = root_access_for(state, session_id, &session_arc, args, password, true).await?;
    // The turn ends once the tool has answered the password (root_feed), not
    // when the command does.
    let turn = password_turn(state, session_id, &access, typed).await?;
    let hook = TurnHook { turn, state, session_id, access: &access };
    let ran = root_run(&session_arc, &access, args, timeout_secs, Some(hook)).await;
    // Where the tool ran the command itself, its refusal is in the output.
    let ran = match ran {
        Ok((code, out)) if code != 0 && access.shell.is_none() => match tool_answer(&out) {
            // `-l` answers without a password where any of the user's rules
            // has NOPASSWD (sudo's `listpw`), even for a command that needs
            // one; running it says so. Then the tab asks for the password.
            Some(a) if a == SUDO_NEED_PASSWORD && access.password.is_none() => {
                forget_root_access(state, session_id, &access).await;
                return Err(a);
            }
            // A kept password's tool, whose rules don't cover this command.
            Some(a) if a == SUDO_NOT_ALLOWED && access.password.is_some() && !typed => Err(a),
            _ => Ok((code, out)),
        },
        ran => ran,
    };
    match ran {
        Ok((code, out)) => Ok((code, out, access.tool)),
        // A kept root shell that didn't work this time (not root any more),
        // or a kept password's tool that doesn't allow this command: look
        // again, with the kept password if there is one. Not after a time-out:
        // the command may have done its work, and isn't run twice.
        Err(e) if !typed && e != SUDO_WRONG_PASSWORD && !e.starts_with(ROOT_TIMED_OUT) && (access.shell.is_some() || e == SUDO_NOT_ALLOWED) => {
            crate::applog::write_line("root", &format!("what the tab had for root didn't work this time ({}); looking again", e));
            forget_root_access(state, session_id, &access).await;
            let again = access.password.as_ref().map(|p| p.to_string());
            let access = root_access_for(state, session_id, &session_arc, args, again, false).await?;
            let (code, out) = root_run(&session_arc, &access, args, timeout_secs, None).await?;
            Ok((code, out, access.tool))
        }
        Err(e) => Err(e),
    }
}

/// Root over a real SSH connection to a server that runs commands with this
/// machine's shell (tcsh as the login shell where it's installed), with
/// stand-ins for sudo and dzdo: `dzdo su -` with a password (#96), what a
/// wrong one costs, and a NOPASSWD rule never getting the password. The AI's
/// root commands are tested on the same stand-ins (ai/tests.rs).
#[cfg(all(test, unix))]
pub(crate) mod root_over_ssh_tests {
    use super::*;
    use crate::ssh_test_server::{connect, ShellOpts, TestClient, TestServer};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// sudo or dzdo, as far as the app can tell. Files beside it set it up:
    /// `rules.<name>` (`all`, `shell` for `su -` only, `none`, plus
    /// `nopasswd`, or `cmdnopw`: a password to sign in, none to run a
    /// command), `password`; `notickets` (no ticket kept after a sign-in,
    /// as with `timestamp_timeout=0`); `pamprompt` (PAM's own prompt shown
    /// instead of `-p`'s); `early` (a warning of its own first, then that
    /// many seconds before it asks); `slowcheck` (seconds to check a
    /// password); `german` (its words in German, but in the C locale) or
    /// `germanalways`; `globaltickets` (a sign-in's ticket counts for every
    /// caller, `ticket.global`, as with `timestamp_type=global`). Each password read is a line in `attempts`. Like sudo
    /// without a terminal, a sign-in leaves a ticket for the caller's
    /// process (its parent), which `-n` and `-v` then use; `-k` ignores it.
    pub(crate) const FAKE_TOOL: &str = r#"#!/bin/sh
D=$(dirname "$0")
T=$(basename "$0")
n=; p=; l=; v=; k=
while [ $# -gt 0 ]; do
  case $1 in
    -n) n=1;; -l) l=1;; -v) v=1;; -k) k=1;; -p) shift; p=$1;; -S) ;; *) break;;
  esac
  shift
done
de=
if [ -f "$D/germanalways" ] || { [ -f "$D/german" ] && [ "$LC_ALL" != C ]; }; then de=1; fi
say() { if [ -n "$de" ]; then echo "$2" >&2; else echo "$1" >&2; fi; }
if [ -f "$D/early" ]; then echo "$T: unable to resolve host test: Name or service not known" >&2; sleep "$(cat "$D/early")"; fi
rules=$(cat "$D/rules.$T")
case $rules in
  *none*) [ -n "$l" ] && exit 1; echo "Sorry, user bob may not run $T on test." >&2; exit 1;;
esac
need=1
case $rules in *nopasswd*) need=;; esac
case $rules in *cmdnopw*) [ -z "$v" ] && need=;; esac
if [ -n "$need" ] && [ -z "$k" ] && { [ -f "$D/ticket.$PPID" ] || [ -f "$D/ticket.global" ]; }; then need=; fi
if [ -n "$need" ]; then
  if [ -n "$n" ]; then say "$T: a password is required" "$T: Ein Passwort ist notwendig"; exit 1; fi
  if [ -f "$D/pamprompt" ]; then p=$(cat "$D/pamprompt"); fi
  tries=0
  while :; do
    printf '%s' "$p" >&2
    IFS= read -r pw || { echo >&2; say "$T: no password was provided" "$T: Es wurde kein Passwort angegeben"; exit 1; }
    echo >&2
    echo try >> "$D/attempts"
    if [ -f "$D/slowcheck" ]; then sleep "$(cat "$D/slowcheck")"; fi
    [ "$pw" = "$(cat "$D/password")" ] && break
    say "Sorry, try again." "Entschuldigung, versuchen Sie es noch einmal."
    tries=$((tries + 1))
    if [ $tries -ge 3 ]; then say "$T: 3 incorrect password attempts" "$T: 3 Fehlversuche bei der Passwort-Eingabe"; exit 1; fi
  done
  if [ -z "$k" ] && [ ! -f "$D/notickets" ]; then touch "$D/ticket.$PPID"; if [ -f "$D/globaltickets" ]; then touch "$D/ticket.global"; fi; fi
fi
[ -n "$v" ] && exit 0
case "$rules" in
  *all*) ;;
  *shell*) if [ "$*" != "su -" ]; then
      [ -n "$l" ] && exit 1
      echo "Sorry, user bob is not allowed to execute '$*' as root on test." >&2; exit 1
    fi;;
esac
[ -n "$l" ] && exit 0
# A real `su -` is root: in its shell `id -u` says 0, whoever runs the tests.
if [ "$1" = su ]; then PATH="$D/root:$PATH" exec sh; fi
exec "$@"
"#;

    pub(crate) struct Lab {
        pub(crate) dir: PathBuf,
        pub(crate) handle: HandleArc<TestClient>,
        /// The server's count of commands still running.
        pub(crate) running: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Lab {
        /// A server whose dzdo and sudo follow `dzdo` and `sudo` (FAKE_TOOL's
        /// rules), with the password `s3cret pass`.
        pub(crate) async fn new(name: &str, dzdo: &str, sudo: &str) -> Lab {
            Lab::new_with_path(name, dzdo, sudo, None).await
        }

        /// The same, with `first` first on the login's PATH.
        pub(crate) async fn new_with_path(name: &str, dzdo: &str, sudo: &str, first: Option<&std::path::Path>) -> Lab {
            let dir = std::env::temp_dir().join(format!("sub-root-{}-{}", name, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for tool in ["dzdo", "sudo"] {
                let path = dir.join(tool);
                std::fs::write(&path, FAKE_TOOL).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            std::fs::create_dir_all(dir.join("root")).unwrap();
            std::fs::write(dir.join("root/id"), "#!/bin/sh\nif [ \"$1\" = -u ]; then echo 0; else exec /usr/bin/env -u PATH id \"$@\"; fi\n").unwrap();
            std::fs::set_permissions(dir.join("root/id"), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(dir.join("rules.dzdo"), dzdo).unwrap();
            std::fs::write(dir.join("rules.sudo"), sudo).unwrap();
            std::fs::write(dir.join("password"), "s3cret pass\n").unwrap();
            // The account's login shell: tcsh where this machine has it.
            let tcsh = std::process::Command::new("tcsh").arg("-c").arg("true").output().is_ok_and(|o| o.status.success());
            let login = if tcsh { vec!["tcsh".to_string(), "-c".to_string()] } else { Vec::new() };
            let opts = ShellOpts {
                path: Some(format!(
                    "{}{}:{}",
                    first.map(|f| format!("{}:", f.display())).unwrap_or_default(),
                    dir.display(),
                    std::env::var("PATH").unwrap_or_default()
                )),
                cwd: Some(dir.clone()),
                login,
                keep_running_on_close: false,
            };
            let server = TestServer::with_shell(opts);
            let running = server.running.clone();
            let session = connect(server, |_| {}, build_ssh_client_config()).await.expect("in-process SSH connection");
            Lab { dir, handle: Arc::new(tokio::sync::RwLock::new(session)), running }
        }

        pub(crate) fn set(&self, file: &str, value: &str) {
            std::fs::write(self.dir.join(file), value).unwrap();
        }

        /// The passwords the tools have read.
        pub(crate) fn attempts(&self) -> usize {
            std::fs::read_to_string(self.dir.join("attempts")).map(|s| s.lines().count()).unwrap_or(0)
        }
    }

    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn pw(s: &str) -> zeroize::Zeroizing<String> {
        zeroize::Zeroizing::new(s.to_string())
    }

    fn words(script: &str) -> Vec<String> {
        vec!["sh".to_string(), "-c".to_string(), script.to_string()]
    }

    fn su(password: &str) -> ssh_manager::RootAccess {
        ssh_manager::RootAccess::new("dzdo", Some(pw(password)), Some("su -"), None)
    }

    #[tokio::test]
    async fn dzdo_su_with_a_password_gives_root() {
        let lab = Lab::new("su", "shell", "none").await;
        let cmd = words("echo ran as root");
        // Without the password: dzdo wants it, and no root shell goes without.
        assert_eq!(root_probe(&lab.handle, "dzdo", None, &cmd, false).await, Err(SUDO_NEED_PASSWORD.to_string()));
        assert_eq!(find_root_shell(&lab.handle, &["dzdo"], false).await, None);
        assert_eq!(lab.attempts(), 0);
        // With it: the command itself isn't allowed, `dzdo su -` is.
        assert_eq!(root_probe(&lab.handle, "dzdo", Some("s3cret pass"), &cmd, false).await, Err(SUDO_NOT_ALLOWED.to_string()));
        let found = find_root_shell_with_password(&lab.handle, &["dzdo"], &pw("s3cret pass"), false).await;
        assert_eq!(found, Ok(Some(("dzdo", "su -"))));
        assert_eq!(lab.attempts(), 2);
        // A command through it: one sign-in, then the ticket.
        let (code, out) = root_run(&lab.handle, &su("s3cret pass"), &cmd, 30, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "ran as root"));
        assert_eq!(lab.attempts(), 3);
    }

    #[tokio::test]
    async fn a_wrong_password_is_tried_once() {
        let lab = Lab::new("wrong", "shell", "none").await;
        let found = find_root_shell_with_password(&lab.handle, &["dzdo"], &pw("not it"), false).await;
        assert_eq!(found, Err(SUDO_WRONG_PASSWORD.to_string()));
        // Not the root shell's line as a second try, not the next form.
        assert_eq!(lab.attempts(), 1);
        assert_eq!(root_run(&lab.handle, &su("not it"), &["true".to_string()], 30, None).await, Err(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(lab.attempts(), 2);
    }

    /// Where the tool asks for nothing (a NOPASSWD rule), the password never
    /// reaches the command (#96).
    #[tokio::test]
    async fn a_command_the_tool_runs_without_asking_never_gets_the_password() {
        let lab = Lab::new("nopasswd", "all nopasswd", "none").await;
        let access = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None);
        // It prints whatever comes on its stdin.
        let (code, out) = root_run(&lab.handle, &access, &words("printf '[%s]' \"$(cat)\""), 30, None).await.unwrap();
        assert_eq!((code, out.as_str()), (0, "[]"));
        assert_eq!(lab.attempts(), 0);
    }

    /// PAM's own prompt ("Password for bob@REALM:", an OTP) instead of the
    /// tool's: the password is checked without looking for any prompt, so
    /// it works; where no ticket is kept, it fails plainly, after one
    /// sign-in, with nothing typed into the prompt it doesn't know (#96).
    #[tokio::test]
    async fn a_prompt_of_pams_own() {
        let lab = Lab::new("pam", "shell", "none").await;
        lab.set("pamprompt", "Password for bob@EXAMPLE.COM: ");
        let (code, out) = root_run(&lab.handle, &su("s3cret pass"), &words("echo root"), 30, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "root"));
        assert_eq!(lab.attempts(), 1);
        // A wrong one under that prompt is said so, after one try.
        assert_eq!(root_run(&lab.handle, &su("not it"), &words("echo root"), 30, None).await, Err(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(lab.attempts(), 2);
        lab.set("notickets", "");
        let got = root_run(&lab.handle, &su("s3cret pass"), &words("echo root"), 60, None).await;
        assert!(got.as_ref().is_err_and(|e| e.contains("prompt of its own")), "{got:?}");
        assert_eq!(lab.attempts(), 3);
    }

    /// A `dzdo` of the login's own, first on its PATH (a program running as
    /// the login put it there), never gets the password: the tool is found
    /// on a fixed PATH.
    #[tokio::test]
    async fn a_tool_planted_on_the_logins_path_is_never_used() {
        let planted = std::env::temp_dir().join(format!("sub-root-planted-{}", std::process::id()));
        std::fs::create_dir_all(&planted).unwrap();
        let caught = planted.join("caught");
        std::fs::write(planted.join("dzdo"), format!("#!/bin/sh\ncat > '{}'\n", caught.display())).unwrap();
        std::fs::set_permissions(planted.join("dzdo"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let lab = Lab::new_with_path("planted", "shell", "none", Some(&planted)).await;
        let (code, out) = root_run(&lab.handle, &su("s3cret pass"), &words("echo root"), 30, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "root"));
        assert!(!caught.exists(), "the planted dzdo ran");
        let _ = std::fs::remove_dir_all(&planted);
    }

    /// A tool its group or anyone else can change, or in a folder they can
    /// change, is never given a password: nothing is read, and the call
    /// says why.
    #[tokio::test]
    async fn a_tool_others_can_change_never_gets_the_password() {
        for what in ["file", "folder"] {
            let lab = Lab::new(&format!("unsafe-{what}"), "all", "none").await;
            let path = if what == "file" { lab.dir.join("dzdo") } else { lab.dir.clone() };
            let mode = if what == "file" { 0o775 } else { 0o777 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let access = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None);
            assert_eq!(root_run(&lab.handle, &access, &words("echo root"), 30, None).await, Err(SUDO_UNSAFE.to_string()), "{what}");
            assert_eq!(root_probe(&lab.handle, "dzdo", Some("s3cret pass"), &words("true"), false).await, Err(SUDO_UNSAFE.to_string()), "{what}");
            assert_eq!(lab.attempts(), 0, "{what}");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// A command that runs without asking, where no ticket is kept, and
    /// prints something and then the app's prompt (read from the script's
    /// environment): the prompt isn't answered, since it isn't the first
    /// thing, and the command never gets the password.
    #[tokio::test]
    async fn output_before_the_prompt_never_gets_the_password() {
        let lab = Lab::new("fakeprompt", "cmdnopw", "none").await;
        lab.set("notickets", "");
        let access = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None);
        let script = r#"printf 'hello\n'; tr '\0' '\n' < /proc/$PPID/environ | sed -n 's/^SUB_TAG=\(.*\)$/\1:PW:/p' | tr -d '\n'; IFS= read -r x; printf '[%s]' "$x" > got"#;
        let got = root_run(&lab.handle, &access, &words(script), 30, None).await;
        assert!(got.as_ref().is_err_and(|e| e.contains("didn't send it")), "{got:?}");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let read = std::fs::read_to_string(lab.dir.join("got")).unwrap_or_default();
        assert!(!read.contains("s3cret"), "{read}");
        assert_eq!(lab.attempts(), 1, "only the check");
    }

    /// Where no ticket is kept, the tool asks again with the app's prompt,
    /// and only that prompt is answered.
    #[tokio::test]
    async fn without_tickets_the_tools_own_ask_is_answered() {
        let lab = Lab::new("notickets", "shell", "none").await;
        lab.set("notickets", "");
        let (code, out) = root_run(&lab.handle, &su("s3cret pass"), &words("echo root"), 30, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "root"));
        assert_eq!(lab.attempts(), 2);
    }

    /// A warning of the tool's own first ("unable to resolve host"), and a
    /// slow sssd or LDAP before it asks: nothing is sent before it does.
    #[tokio::test]
    async fn an_early_warning_and_a_slow_ask() {
        let lab = Lab::new("early", "shell", "none").await;
        lab.set("early", "2");
        let (code, out) = root_run(&lab.handle, &su("s3cret pass"), &words("echo root"), 60, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "root"));
        assert_eq!(lab.attempts(), 1);
    }

    /// Sudo's tickets, shared across terminals (`timestamp_type=global`):
    /// the server tab's own root may use one, the AI's never does, and the
    /// AI's own sign-in leaves none behind.
    #[tokio::test]
    async fn the_ais_root_neither_uses_nor_leaves_a_ticket() {
        let lab = Lab::new("tickets", "all", "none").await;
        lab.set("globaltickets", "");
        lab.set("ticket.global", "");
        let cmd = words("echo root");
        assert_eq!(root_probe(&lab.handle, "dzdo", None, &cmd, false).await, Ok(()));
        assert_eq!(root_probe(&lab.handle, "dzdo", None, &cmd, true).await, Err(SUDO_NEED_PASSWORD.to_string()));
        assert_eq!(find_root_without_password(&lab.handle, &cmd, true).await.err().map(|e| e.0), Some(SUDO_NEED_PASSWORD.to_string()));
        let ai = ssh_manager::RootAccess::new("dzdo", None, None, None).without_tickets();
        let (code, out) = root_run(&lab.handle, &ai, &cmd, 30, None).await.unwrap();
        assert!(code != 0 && out.contains("password is required"), "{code} {out}");
        std::fs::remove_file(lab.dir.join("ticket.global")).unwrap();
        let ai = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None).without_tickets();
        let (code, out) = root_run(&lab.handle, &ai, &cmd, 30, None).await.unwrap();
        assert_eq!((code, out.trim()), (0, "root"));
        assert!(!lab.dir.join("ticket.global").exists(), "the AI's sign-in left a ticket");
        // The tab's own root does leave one, as sudo does.
        let tab = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None);
        root_run(&lab.handle, &tab, &cmd, 30, None).await.unwrap();
        assert!(lab.dir.join("ticket.global").exists());
    }

    /// A sudo that speaks German is asked in English (the C locale), so a
    /// wrong password is seen as one, once.
    #[tokio::test]
    async fn a_tool_in_another_language_is_asked_in_english() {
        let lab = Lab::new("german", "all", "none").await;
        lab.set("german", "");
        assert_eq!(root_probe(&lab.handle, "dzdo", Some("not it"), &words("true"), false).await, Err(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(root_probe(&lab.handle, "dzdo", None, &words("true"), false).await, Err(SUDO_NEED_PASSWORD.to_string()));
        let wrong = ssh_manager::RootAccess::new("dzdo", Some(pw("not it")), None, None);
        assert_eq!(root_run(&lab.handle, &wrong, &words("echo root"), 30, None).await, Err(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(lab.attempts(), 2);
    }

    /// A slow answer to a wrong password (AD): still one attempt, and the
    /// root shell's line never becomes a second.
    #[tokio::test]
    async fn a_slow_answer_to_a_wrong_password_is_one_attempt() {
        let lab = Lab::new("slow", "shell", "none").await;
        lab.set("slowcheck", "7");
        assert_eq!(root_run(&lab.handle, &su("not it"), &words("echo root"), 60, None).await, Err(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(lab.attempts(), 1);
    }

    /// SFTP root mode through `dzdo su -` with the password.
    #[tokio::test]
    async fn sftp_root_mode_finds_dzdo_su_with_a_password() {
        let lab = Lab::new("sftp", "shell", "none").await;
        // This machine may have no sftp-server: then that's the answer.
        let have_server = std::process::Command::new("sh")
            .arg("-c")
            .arg(SFTP_SERVER_FIND)
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("PATH:"));
        let got = probe_sudo_sftp(&lab.handle, Some(&pw("s3cret pass"))).await;
        if have_server {
            let (_, tool, shell) = got.unwrap();
            assert_eq!((tool, shell), ("dzdo", Some("su -")));
        } else {
            assert_eq!(got, Err(SUDO_NO_SFTP_SERVER.to_string()));
        }
    }

    /// Files as root with the password, through `dzdo su -` and through
    /// `dzdo <sftp-server>`: SFTP starts and answers, and a wrong password
    /// is said so after one try (#96).
    #[tokio::test]
    async fn sftp_root_mode_opens_and_says_when_the_password_is_wrong() {
        for (rules, shell) in [("shell", Some("su -")), ("all", None)] {
            let lab = Lab::new(&format!("sftp-open-{}", rules), rules, "none").await;
            let Ok(path) = find_sftp_server(&lab.handle).await else {
                return; // No sftp-server on this machine.
            };
            let elevation = ssh_manager::SftpElevation {
                server_path: Some(path.clone()),
                tool: "dzdo",
                password: Some(pw("s3cret pass")),
                shell,
                password_failed: false,
            };
            let sftp = open_elevated_sftp(&lab.handle, &path, &elevation).await.unwrap();
            assert!(sftp.canonicalize(".").await.unwrap().starts_with('/'));
            let before = lab.attempts();
            let wrong = ssh_manager::SftpElevation { password: Some(pw("not it")), ..elevation };
            assert_eq!(open_elevated_sftp(&lab.handle, &path, &wrong).await.err(), Some(SUDO_WRONG_PASSWORD.to_string()), "{rules}");
            assert_eq!(lab.attempts(), before + 1, "{rules}");
        }
    }

    async fn read_all(mut channel: russh::Channel<russh::client::Msg>, early: Vec<u8>) -> String {
        let mut all = early;
        while let Some(msg) = channel.wait().await {
            match msg {
                russh::ChannelMsg::Data { ref data } => all.extend_from_slice(data),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    }

    /// A container's live logs as root with the password, a stopped
    /// container's that end at once among them; a wrong password is caught
    /// before the logs start.
    #[tokio::test]
    async fn a_root_stream_with_a_password() {
        let lab = Lab::new("stream", "all", "none").await;
        let access = ssh_manager::RootAccess::new("dzdo", Some(pw("s3cret pass")), None, None);
        let (channel, marker, early) = root_channel_open(&lab.handle, &access, &words("echo one; sleep 1; echo two"), None, None).await.unwrap();
        assert_eq!(marker, None);
        assert_eq!(read_all(channel, early).await, "one\ntwo\n");
        let (channel, _, early) = root_channel_open(&lab.handle, &access, &words("echo the last line"), None, None).await.unwrap();
        assert_eq!(read_all(channel, early).await, "the last line\n");
        let wrong = ssh_manager::RootAccess::new("dzdo", Some(pw("not it")), None, None);
        assert_eq!(root_channel_open(&lab.handle, &wrong, &words("echo one"), None, None).await.err(), Some(SUDO_WRONG_PASSWORD.to_string()));
        assert_eq!(lab.attempts(), 3);
    }

    /// The password typed once and handed to every call of the tab that was
    /// waiting for it: a mistyped one is tried once, a right one signs in
    /// once (#96).
    #[tokio::test]
    async fn a_typed_password_is_tried_once_for_all_the_waiting_calls() {
        let lab = Arc::new(Lab::new("typed", "all", "none").await);
        let state = Arc::new(SshState::new());
        for (typed, attempts) in [("not it", 1), ("s3cret pass", 2)] {
            let mut calls = Vec::new();
            for _ in 0..4 {
                let (lab, state) = (Arc::clone(&lab), Arc::clone(&state));
                calls.push(tokio::spawn(async move {
                    typed_root_access(&state, "tab", &lab.handle, &words("true"), pw(typed)).await.map(|a| a.tool)
                }));
            }
            for call in calls {
                let got = call.await.unwrap();
                if typed == "not it" {
                    assert_eq!(got, Err(SUDO_WRONG_PASSWORD.to_string()));
                } else {
                    assert_eq!(got, Ok("dzdo"));
                }
            }
            assert_eq!(lab.attempts(), attempts, "{typed}");
        }
    }

    /// A kept password that stopped working: one call tries it, the tab's
    /// others find it forgotten (root access and SFTP root mode alike).
    #[tokio::test]
    async fn a_kept_password_that_stopped_working_is_tried_once() {
        let state = SshState::new();
        let access = ssh_manager::RootAccess::new("dzdo", Some(pw("old")), None, None);
        state.root_access.lock().await.insert("tab".into(), access.clone());
        state.sftp_elevation.lock().await.insert(
            "tab".into(),
            ssh_manager::SftpElevation { server_path: None, tool: "dzdo", password: Some(pw("old")), shell: None, password_failed: false },
        );
        let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let state = Arc::new(state);
        let mut calls = Vec::new();
        for _ in 0..4 {
            let (state, access, tries) = (Arc::clone(&state), access.clone(), Arc::clone(&tries));
            calls.push(tokio::spawn(async move {
                let turn = password_turn(&state, "tab", &access, false).await?;
                tries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                turn.finish(&state, "tab", &access, Some(false)).await;
                Err::<(), String>(SUDO_WRONG_PASSWORD.into())
            }));
        }
        for call in calls {
            assert_eq!(call.await.unwrap(), Err(SUDO_WRONG_PASSWORD.to_string()));
        }
        assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(state.root_access.lock().await.get("tab").is_none());
        assert!(state.sftp_elevation.lock().await.get("tab").unwrap().password_failed);
        assert!(known_root_access(&state, "tab").await.is_none());
    }

    /// One that works: after the first call's answer, the others go at once.
    #[tokio::test]
    async fn a_kept_password_that_works_is_trusted_after_one_answer() {
        let state = SshState::new();
        let access = ssh_manager::RootAccess::new("sudo", Some(pw("ok")), None, None);
        state.root_access.lock().await.insert("tab".into(), access.clone());
        let first = password_turn(&state, "tab", &access, false).await.unwrap();
        assert!(first.held.is_some());
        first.finish(&state, "tab", &access, Some(true)).await;
        let next = password_turn(&state, "tab", &access, false).await.unwrap();
        assert!(next.held.is_none());
    }
}

// POSIX sh, sent with remote_sh::sh_command (any login shell passes it on
// unread). `SUB_MODE=pw`
// makes sudo read the password from stdin; `-k` ignores a cached ticket so
// the password is really checked. `sudo -l <cmd>` tests the exact command
// we will run, which also works for sudoers rules scoped to sftp-server.
macro_rules! sftp_server_find {
    () => {
        r#"S=
for p in /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server /usr/lib/ssh/sftp-server /usr/libexec/sftp-server /usr/lib/sftp-server /usr/local/libexec/sftp-server /usr/local/lib/sftp-server /usr/lib64/misc/sftp-server; do
  if [ -x "$p" ]; then S=$p; break; fi
done
if [ -z "$S" ] && [ -r /etc/ssh/sshd_config ]; then
  while read -r k n v rest; do
    case "$k" in [Ss]ubsystem) if [ "$n" = sftp ]; then case "$v" in /*) if [ -x "$v" ]; then S=$v; fi;; esac; fi;; esac
  done < /etc/ssh/sshd_config
fi
if [ -z "$S" ]; then echo __SUB_SUDO:NO_SFTP_SERVER; exit 0; fi
echo "__SUB_SUDO:PATH:$S""#
    };
}

/// Finds the server's sftp-server, for a root shell to start (find_sftp_server).
pub(crate) const SFTP_SERVER_FIND: &str = sftp_server_find!();
/// Finds sftp-server and asks the tool about it, as ROOT_PROBE_SCRIPT does
/// (root_tool_find; with a password, `READ` once the tool is safe).
pub(crate) const SUDO_SFTP_PROBE: &str = concat!(
    root_tool_find!(),
    "\n",
    sftp_server_find!(),
    "\n",
    r#"if [ -z "$X" ]; then echo __SUB_SUDO:NO_SUDO; exit 0; fi
if [ "$SUB_MODE" = pw ]; then
if safe; then :; else echo __SUB_SUDO:UNSAFE; exit 0; fi
echo __SUB_SUDO:READ
OUT=$(LC_ALL=C LANG=C "$X" -S -k -p "" -l "$S" 2>&1)
else OUT=$(LC_ALL=C LANG=C "$X" -n -l "$S" 2>&1); fi
if [ $? -eq 0 ]; then echo __SUB_SUDO:OK; else echo "__SUB_SUDO:FAIL:$(printf %s "$OUT" | tr "\n" " ")"; fi"#
);

/// The command that runs SUDO_SFTP_PROBE for `tool`.
pub(crate) fn sftp_probe_command(tool: &str, password: bool) -> String {
    let mut env = root_tool_env(tool);
    env.push(("SUB_MODE", if password { "pw" } else { "n" }.to_string()));
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    remote_sh::sh_command_with("/bin/sh", &env, SUDO_SFTP_PROBE, &[])
}

/// Programs that can run sftp-server as root, in the order they're tried.
/// `dzdo` first: a host that has it is managed through it, and sudo there is
/// often present but not set up for the user. Hosts without dzdo answer
/// NO_SUDO for it at once and go on to sudo.
pub(crate) const ELEVATION_TOOLS: &[&str] = &["dzdo", "sudo"];

/// The error worth showing when no tool worked: one the user can act on
/// before "not allowed", and "not allowed" before "not installed".
pub(crate) fn elevation_error_rank(e: &str) -> u8 {
    if e == SUDO_NO_SUDO {
        0
    } else if e == SUDO_NOT_ALLOWED {
        1
    } else if e == SUDO_NEED_PASSWORD {
        3
    } else {
        2
    }
}

/// Find sftp-server and a tool (ELEVATION_TOOLS) that may run it as root.
/// Returns the sftp-server path and the tool.
///
/// Without a password every tool gets a try, so a passwordless sudo isn't
/// missed behind a dzdo that wants one. With one, a tool that isn't there or
/// doesn't allow the user hands over to the next, and any other answer (a
/// wrong password above all) ends it, so a mistyped password counts as one
/// failed attempt, not one per tool.
///
/// When no tool may run sftp-server itself, a root shell is tried next:
/// without a password (find_root_shell), `dzdo su -` with no password is
/// how many AD-managed hosts give root, and must not end in a password
/// prompt; with one (find_root_shell_with_password), `dzdo su -` that asks
/// for it. Returns the root shell's words too, when that's the way in.
pub(crate) async fn probe_sudo_sftp<H: russh::client::Handler>(
    session_arc: &HandleArc<H>,
    password: Option<&zeroize::Zeroizing<String>>,
) -> Result<(String, &'static str, Option<&'static str>), String> {
    let mut best: Option<String> = None;
    let mut refused: Vec<&'static str> = Vec::new();
    for tool in ELEVATION_TOOLS {
        match probe_sudo_sftp_with(session_arc, password.map(|p| p.as_str()), tool).await {
            Ok(path) => return Ok((path, tool, None)),
            // A fact about the server, whichever tool asked.
            Err(e) if e == SUDO_NO_SFTP_SERVER => return Err(e),
            Err(e) if password.is_some() && e != SUDO_NO_SUDO && e != SUDO_NOT_ALLOWED => return Err(e),
            Err(e) => {
                if e == SUDO_NOT_ALLOWED {
                    refused.push(tool);
                }
                if e != SUDO_NO_SUDO {
                    crate::applog::write_line("root", &format!("SFTP as root: {} may not run sftp-server itself ({})", tool, e));
                }
                if best.as_deref().is_none_or(|b| elevation_error_rank(&e) > elevation_error_rank(b)) {
                    best = Some(e);
                }
            }
        }
    }
    let shell = match password {
        None => find_root_shell(session_arc, &refused, false).await,
        Some(pw) => find_root_shell_with_password(session_arc, &refused, pw, false).await?,
    };
    if let Some((tool, form)) = shell {
        let path = find_sftp_server(session_arc).await?;
        crate::applog::write_line("root", &format!("SFTP as root: through `{} {}`", tool, form));
        return Ok((path, tool, Some(form)));
    }
    Err(best.unwrap_or_else(|| SUDO_NO_SUDO.into()))
}

/// The server's sftp-server, found as the login user.
pub(crate) async fn find_sftp_server<H: russh::client::Handler>(session_arc: &HandleArc<H>) -> Result<String, String> {
    let cmd = remote_sh::sh_command(&[], SFTP_SERVER_FIND, &[]);
    let out = exec_with_stdin_capture(session_arc, &cmd, None, 20).await?;
    for line in out.lines() {
        match line.trim().strip_prefix("__SUB_SUDO:") {
            Some("NO_SFTP_SERVER") => return Err(SUDO_NO_SFTP_SERVER.into()),
            Some(rest) => {
                if let Some(p) = rest.strip_prefix("PATH:") {
                    let p = p.trim();
                    if !is_safe_remote_exec_path(p) {
                        return Err(format!("{}: unusual sftp-server path", SUDO_FAILED));
                    }
                    return Ok(p.to_string());
                }
            }
            None => {}
        }
    }
    Err(format!("{}: unexpected reply from the server", SUDO_FAILED))
}

// Every way the original project's code reaches root goes through here
// (fork_hooks.rs): a sync that drops one of these calls, or the lines that
// bring this file in, fails here.
#[cfg(test)]
mod hook_tests {
    use crate::fork_hooks::assert_hooks;

    #[test]
    fn the_hooks_are_in_place() {
        assert_hooks(
            "lib.rs",
            include_str!("lib.rs"),
            &[
                ("", "mod root_ssh;", 1),
                ("", "use root_ssh::*;", 1),
                ("ssh_systemctl_action", "run_as_root(", 1),
                ("ssh_kill_process", "run_as_root(", 1),
                ("ssh_iptables_chain", "run_as_root(", 1),
                ("ssh_nft_chain", "run_as_root(", 1),
                ("probe_sudo_sftp_with", "exec_probe_with_password(", 1),
                ("probe_sudo_sftp_with", "sftp_probe_command(", 1),
                ("probe_sudo_files", "probe_sudo_sftp(", 1),
                ("open_elevated_sftp", "ELEVATION_TOOLS", 1),
                ("probe_sudo_files", "password_line(", 1),
                ("open_elevated_sftp", "root_feed(", 1),
                ("sftp_set_elevated", "just_rejected(", 1),
                ("get_sftp_session", "forget_root_password(", 1),
            ],
        );
        assert_hooks(
            "docker.rs",
            include_str!("docker.rs"),
            &[
                ("run_docker_smart", "run_as_root(", 1),
                ("ssh_docker_logs_start", "root_channel(", 1),
                ("open_container_terminal", "root_channel(", 1),
            ],
        );
    }
}

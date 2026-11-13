//! The fork's own tests in `elevated_sftp_tests`, kept in a file of their own (#136) so
//! a sync with the original project doesn't conflict on them. A child of
//! `elevated_sftp_tests`, so they use its helpers as they are.

use super::*;

/// What a command prints is never taken for the tool's answer (#96).
#[test]
fn only_the_tools_own_lines_are_read() {
    // A container's log, a unit's status text.
    let noise = "app: incorrect password for user admin\nauth: a password is required\nlogin: authentication failure\n";
    assert_eq!(classify_tool_output(noise), SUDO_NOT_ALLOWED);
    assert_eq!(classify_tool_output(&format!("{noise}sudo: a password is required\n")), SUDO_NEED_PASSWORD);
    assert_eq!(classify_tool_output("Sorry, try again.\r\nSorry, try again.\r\ndzdo: 2 incorrect password attempts\r\n"), SUDO_WRONG_PASSWORD);
    assert_eq!(classify_tool_output("  sudo-rs: interactive authentication is required"), SUDO_NEED_PASSWORD);
    assert_eq!(classify_tool_output("bob is not in the sudoers file.  This incident will be reported.\n"), SUDO_NOT_ALLOWED);
    assert_eq!(
        classify_tool_output("Last login: today\nSorry, user bob is not allowed to execute '/bin/su -' as root on web1.\n"),
        SUDO_NOT_ALLOWED
    );
    assert_eq!(classify_tool_output("sudo: sorry, you must have a tty to run sudo\n"), SUDO_REQUIRETTY);
}

/// sudo and dzdo are looked for in the system's folders, then in
/// Centrify's (#232), which are never on the scripts' PATH.
#[test]
fn centrifys_folders_come_after_the_systems() {
    assert_eq!(ROOT_TOOL_EXTRA_PATH, "/usr/share/centrifydc/bin:/opt/centrify/bin");
    assert!(!ROOT_TOOL_PATH.contains("centrify"));
    assert!(root_tool_extra_path().ends_with(ROOT_TOOL_EXTRA_PATH));
    let env = root_tool_env("dzdo");
    let order: Vec<&str> = env.iter().map(|(k, _)| *k).collect();
    assert_eq!(order, ["SUB_TOOL", "SUB_PATH", "SUB_XPATH", "SUB_UID"]);
    assert_eq!(ELEVATION_TOOLS, ["dzdo", "sudo"]);
}

#[test]
fn the_most_useful_elevation_error_wins() {
    let best = |errs: &[&str]| errs.iter().copied().max_by_key(|e| elevation_error_rank(e)).unwrap().to_string();
    // dzdo not installed, sudo wants a password: ask for it.
    assert_eq!(best(&[SUDO_NO_SUDO, SUDO_NEED_PASSWORD]), SUDO_NEED_PASSWORD);
    // dzdo refuses the user, sudo not installed: say "not allowed".
    assert_eq!(best(&[SUDO_NOT_ALLOWED, SUDO_NO_SUDO]), SUDO_NOT_ALLOWED);
    // requiretty and friends beat "not allowed".
    assert_eq!(best(&[SUDO_REQUIRETTY, SUDO_NOT_ALLOWED]), SUDO_REQUIRETTY);
}

/// dzdo (Centrify / Delinea) is built from sudo's code and words its
/// answers the same way, after its own name.
#[test]
fn dzdo_errors_map_to_the_same_codes() {
    assert_eq!(classify_sudo_failure("dzdo: a password is required"), SUDO_NEED_PASSWORD);
    assert_eq!(classify_sudo_failure("Sorry, user bob may not run dzdo on host."), SUDO_NOT_ALLOWED);
    assert_eq!(classify_sudo_failure("Sorry, try again. dzdo: 1 incorrect password attempt"), SUDO_WRONG_PASSWORD);
    assert_eq!(
        classify_sudo_failure("Sorry, user bob is not allowed to execute '/usr/libexec/openssh/sftp-server' as root on host."),
        SUDO_NOT_ALLOWED
    );
}

/// Every script the root features send, as the exact command sent, run
/// through sh and through tcsh and csh (a login shell that rejected the
/// scripts' line breaks inside quotes, #96). `true` and `false` stand in
/// for sudo.
#[test]
fn root_scripts_run_under_any_login_shell() {
    let args = vec!["systemctl".to_string(), "restart".to_string(), "it's".to_string()];
    let cases: Vec<(String, &str)> = vec![
        (root_probe_command("true", false, &args, false), "__SUB_SUDO:OK"),
        (root_probe_command("true", false, &args, true), "__SUB_SUDO:OK"),
        (root_probe_command("false", true, &args, false), "__SUB_SUDO:FAIL:"),
        (root_probe_command("no-such-tool", false, &args, false), "__SUB_SUDO:NO_SUDO"),
        (root_exec_command("true", "TAG", false, false, true, false, &args), "TAG:RUN:n"),
        (root_exec_command("true", "TAG", true, false, true, false, &args), "TAG:READ"),
        (root_exec_command("true", "TAG", true, false, true, true, &args), "TAG:READ"),
        (root_exec_command("no-such-tool", "TAG", true, false, true, false, &args), "TAG:NO_TOOL"),
        (root_shell_try_command("no-such-tool", "su -", true), "__SUB_SUDO:NO_SUDO"),
        (sftp_probe_command("true", false), "__SUB_SUDO:"),
        (remote_sh::sh_command(&[], SFTP_SERVER_FIND, &[]), "__SUB_SUDO:"),
        (remote_sh::sh_command(&[("SUB_MODE", "n")], SUDO_SHELL_PROBE, &[]), "__SUB_SUDO:"),
    ];
    for (cmd, _) in &cases {
        assert!(!cmd.contains(['\n', '\r', '!']), "{cmd}");
        // Dropbear takes a command of up to 9000 bytes.
        assert!(cmd.len() < 8000, "{} bytes", cmd.len());
    }
    #[cfg(unix)]
    for shell in shells() {
        for (cmd, want) in &cases {
            let out = std::process::Command::new(shell).arg("-c").arg(cmd).output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains(want), "{shell}: {cmd}\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
        }
    }
}

/// The shells a server may run the app's commands with: its login
/// shell (sshd runs `<shell> -c <command>`) or root's (a root shell
/// reads its line). sh always; tcsh and csh where this machine has them.
#[cfg(unix)]
fn shells() -> Vec<&'static str> {
    ["sh", "tcsh", "csh"]
        .into_iter()
        .filter(|s| std::process::Command::new(s).arg("-c").arg("true").output().is_ok_and(|o| o.status.success()))
        .collect()
}

#[test]
fn a_root_shell_gets_one_line_of_posix_sh() {
    let script = format!("echo __SUB_ROOT_OUT__; printf '%s|' {}; echo", remote_sh::quote_words(&["it's".into(), "a b".into()]));
    let line = root_shell_line(&script);
    assert!(line.ends_with('\n') && line.matches('\n').count() == 1 && !line.contains('!'), "{line:?}");
    for form in ROOT_SHELL_FORMS {
        assert!(form.chars().all(|c| c.is_ascii_lowercase() || c == ' ' || c == '-'), "{form}");
    }
    // What root's shell makes of it, whichever it is.
    #[cfg(unix)]
    for shell in shells() {
        let out = std::process::Command::new(shell)
            .arg("-c")
            .arg(line.trim_end().strip_prefix("exec ").unwrap())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "__SUB_ROOT_OUT__\nit's|a b|\n", "{shell}: {}", String::from_utf8_lossy(&out.stderr));
    }
}

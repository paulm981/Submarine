//! The fork's own tests, kept in a file of their own (#136) so a sync with
//! the original project doesn't conflict on them.

use super::{daemon_refused, docker, Cmd};

/// What a POSIX shell (the server's) makes of a command's words.
fn argv(shell: &str) -> String {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s|' {}", shell.trim_start_matches("'docker' ")))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn each_word_reaches_docker_as_one_argument() {
    let fmt = r#"{{.Names}}|{{.Label "com.docker.compose.project"}}|{{.State}}"#;
    let cmd = docker(&["ps", "-a", "--format", fmt, "it's"]);
    let Cmd::Docker(words) = &cmd else { panic!() };
    assert_eq!(words[0], "docker");
    assert!(cmd.shell().starts_with("'docker' 'ps'"));
    if cfg!(unix) {
        assert_eq!(argv(&cmd.shell()), format!("ps|-a|--format|{}|it's|", fmt));
    }
}

#[test]
fn compose_runs_the_plugin_or_the_old_script() {
    let cmd = Cmd::Compose(vec!["-f".into(), "/srv/my app/compose.yml".into(), "ps".into()]);
    let shell = cmd.shell();
    assert!(shell.ends_with("$CCMD '-f' '/srv/my app/compose.yml' 'ps'"), "{shell}");
}

#[test]
fn refused_by_the_daemon_is_told_from_other_failures() {
    assert!(daemon_refused("permission denied while trying to connect to the docker API at unix:///var/run/docker.sock"));
    assert!(daemon_refused("Got permission denied while trying to connect to the Docker daemon socket"));
    assert!(!daemon_refused("Error response from daemon: No such container: web"));
    // Not root's to fix: the daemon is down, or a container's own error.
    assert!(!daemon_refused("Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"));
    assert!(!daemon_refused("OCI runtime create failed: runc create failed: exec: \"/app/run.sh\": permission denied: unknown"));
}

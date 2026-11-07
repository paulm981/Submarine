//! The fork's own tests in `client_import_tests`, kept in a file of their own (#136) so
//! a sync with the original project doesn't conflict on them. A child of
//! `client_import_tests`, so they use its helpers as they are.

use super::*;

#[test]
fn a_startup_command_comes_back_as_it_was_typed() {
    let text = export(&[session(
        "app",
        109,
        "0%app.example%22%ops%%-1%-1%cd /opt/app__PTVIRG__ ps aux __PIPE__ grep __DBLQUO__java__DBLQUO__ __DIEZE__ 100__PERCENT__%%%%0%0%0%%%-1",
    )]);
    let hosts = parse_mobaxterm_sessions_in(&text, None, None).unwrap();
    assert_eq!(hosts[0].run_on_connect.as_deref(), Some("cd /opt/app; ps aux | grep \"java\" # 100%"));
}

#[test]
fn saved_logins_commands_and_the_jump_hosts_key_carry_over() {
    let text = export(&[
        session("app", 109, r"0%app.example%22%[ops]%%-1%-1%sudo -i%bastion.example%22%jump%-1%0%0%%_ProfileDir_\.ssh\id_jump%-1"),
        session("plain", 109, "0%plain.example%22%alice%%-1%-1%%%%%0%0%0%%%-1"),
        session("odd", 109, "0%odd.example%22%[]%%-1%-1%%%%%0%0%0%%%-1"),
    ]);
    let hosts = parse_mobaxterm_sessions_in(&text, Some(r"C:\Users\me"), None).unwrap();
    let app = &hosts[0];
    assert_eq!((app.user.as_str(), app.credential.as_deref()), ("", Some("ops")));
    assert_eq!(app.run_on_connect.as_deref(), Some("sudo -i"));
    assert_eq!(app.proxy_jump.as_deref(), Some("jump@bastion.example"));
    assert_eq!(app.jump_identity_file.as_deref(), Some(r"C:\Users\me\.ssh\id_jump"));
    assert_eq!(app.jump_credential, None);
    let plain = &hosts[1];
    assert_eq!((plain.user.as_str(), plain.credential.as_deref(), plain.run_on_connect.as_deref()), ("alice", None, None));
    assert_eq!(plain.jump_identity_file, None);
    assert_eq!((hosts[2].user.as_str(), hosts[2].credential.as_deref()), ("[]", None), "an empty [] is not a login");
}

#[test]
fn a_jump_hosts_saved_login_is_not_taken_for_a_user_name() {
    let text = export(&[session("app", 109, "0%app.example%22%[ops]%%-1%-1%%bastion.example%22%[jumplogin]%-1%0%0%%%-1")]);
    let hosts = parse_mobaxterm_sessions_in(&text, None, None).unwrap();
    assert_eq!(hosts[0].proxy_jump.as_deref(), Some("bastion.example"));
    assert_eq!(hosts[0].jump_credential.as_deref(), Some("jumplogin"));
}

#[test]
fn mobaxterm_path_placeholders_expand() {
    let docs = Some(r"C:\Users\me\Documents");
    let home = Some(r"C:\Users\me");
    assert_eq!(expand_mobaxterm_path(r"_CurrentDrive_:\keys\k.pem", home, docs), r"C:\keys\k.pem");
    assert_eq!(expand_mobaxterm_path(r"_ProfileDir_\.ssh\id", home, docs), r"C:\Users\me\.ssh\id");
    assert_eq!(expand_mobaxterm_path(r"_MyDocuments_\MobaXterm\home\.ssh\k", home, docs), r"C:\Users\me\Documents\MobaXterm\home\.ssh\k");
    assert_eq!(expand_mobaxterm_path(r"_ProfileDir_\id", None, None), r"_ProfileDir_\id", "left as is without a folder to put in");
}

#[test]
fn sessions_that_arent_ssh_are_counted() {
    let text = export(&[
        session("desktop", 91, "4%c.example%3389%carol%%-1%0%0"),
        session("desktop2", 91, "4%d.example%3389%carol%%-1%0%0"),
        session("vnc", 128, "5%e.example%5900%%-1%0"),
        session("web", 109, "0%f.example%22%u%%-1%-1%%%%%0%0%0%%%-1"),
    ]);
    assert_eq!(
        mobaxterm_skipped(&text).as_deref(),
        Some("Not imported: 1 VNC session — Submarine imports SSH and RDP sessions.")
    );
    let ssh_only = export(&[session("web", 109, "0%f.example%22%u%%-1%-1%%%%%0%0%0%%%-1")]);
    assert_eq!(mobaxterm_skipped(&ssh_only), None);
}

#[test]
fn ssh_config_import_reads_settings_as_openssh_does() {
    let hosts = crate::parse_ssh_config_text(
        "User everyone\n\
         Host web1 web2\n\
         \tHostName web.example\n\
         \tPort 2200\n\
         \tPort 2300\n\
         Match host web1\n\
         \tUser matched\n\
         \tPort 9999\n\
         Host db\n\
         \tHostName=db.example\n\
         \tProxyJump none\n\
         Host web*\n\
         \tIdentityFile ~/.ssh/web\n\
         \tProxyJump bastion.example\n\
         Host *\n\
         \tUser fallback\n\
         \tPort 2022\n\
         \tProxyJump gate.example\n\
         Host web1\n\
         \tHostName ignored.example\n",
    );
    let names: Vec<&str> = hosts.iter().map(|h| h.host_alias.as_str()).collect();
    assert_eq!(names, vec!["web1", "web2", "db"], "no wildcard host, and web1 once");
    let keys: Vec<&str> = hosts.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys, vec!["0", "1", "2"]);
    let web1 = &hosts[0];
    assert_eq!(web1.hostname, "web.example", "the first HostName wins");
    assert_eq!(web1.port, 2200, "the first Port wins; Match's lines don't count");
    assert_eq!(web1.user, "everyone", "lines before any Host apply to all");
    assert_eq!(web1.identity_file.as_deref(), Some("~/.ssh/web"), "a matching wildcard block gives defaults");
    assert_eq!(web1.proxy_jump.as_deref(), Some("bastion.example"));
    let db = &hosts[2];
    assert_eq!((db.hostname.as_str(), db.port, db.user.as_str()), ("db.example", 2022, "everyone"));
    assert_eq!(db.identity_file, None);
    assert_eq!(db.proxy_jump, None, "ProxyJump none wins over Host *");
}

#[test]
fn ssh_config_import_expands_percent_h() {
    let hosts = crate::parse_ssh_config_text(
        "Host web1 web2 odd\n\
         Host odd\n\
         \tHostName %r.example\n\
         \tIdentityFile ~/.ssh/%u_key\n\
         \tProxyJump %r@gate\n\
         Host web*\n\
         \tHostName %h.prod.example.com\n\
         \tIdentityFile ~/.ssh/%h-100%%\n\
         \tProxyJump gate.example\n",
    );
    let got: Vec<(&str, &str, Option<&str>, Option<&str>)> = hosts
        .iter()
        .map(|h| (h.host_alias.as_str(), h.hostname.as_str(), h.identity_file.as_deref(), h.proxy_jump.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("web1", "web1.prod.example.com", Some("~/.ssh/web1.prod.example.com-100%"), Some("gate.example")),
            ("web2", "web2.prod.example.com", Some("~/.ssh/web2.prod.example.com-100%"), Some("gate.example")),
            ("odd", "odd", None, None),
        ],
        "%h is the alias in HostName and the host in IdentityFile; other tokens are left out"
    );
}

#[test]
fn ssh_config_import_honours_negated_patterns() {
    let hosts = crate::parse_ssh_config_text(
        "Host prod-a prod-b\n\
         Host prod-* !prod-b\n\
         \tUser deploy\n",
    );
    assert_eq!(hosts.len(), 2);
    assert_eq!((hosts[0].user.as_str(), hosts[1].user.as_str()), ("deploy", ""));
}

#[test]
fn ssh_config_jump_aliases_name_the_real_host() {
    let host = |alias: &str, hostname: &str, user: &str, port: u16, jump: Option<&str>| crate::ImportedHost {
        host_alias: alias.into(),
        hostname: hostname.into(),
        user: user.into(),
        port,
        proxy_jump: jump.map(Into::into),
        ..Default::default()
    };
    let mut hosts = vec![
        host("app", "10.0.0.2", "ops", 22, Some("bastion")),
        host("db", "10.0.0.3", "ops", 22, Some("me@other.example:2200")),
        host("deep", "10.0.0.4", "", 22, Some("bastion,inner")),
        host("bastion", "10.0.0.1", "jump", 2222, None),
        host("inner", "10.9.0.1", "", 22, None),
    ];
    crate::resolve_proxy_jump_aliases(&mut hosts);
    assert_eq!(hosts[0].proxy_jump.as_deref(), Some("jump@10.0.0.1:2222"));
    assert_eq!(hosts[1].proxy_jump.as_deref(), Some("me@other.example:2200"), "a spelled-out hop stays as it is");
    assert_eq!(hosts[2].proxy_jump.as_deref(), Some("jump@10.0.0.1:2222,10.9.0.1"));
    assert_eq!(hosts[3].proxy_jump, None);
}

#[test]
fn putty_import_takes_ssh_and_telnet_and_counts_the_rest() {
    let session = |name: &str, host: &str, protocol: Option<&str>, port: Option<u32>| {
        let mut s = format!("[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\{name}]\r\n\"HostName\"=\"{host}\"\r\n");
        if let Some(p) = protocol {
            s.push_str(&format!("\"Protocol\"=\"{p}\"\r\n"));
        }
        if let Some(p) = port {
            s.push_str(&format!("\"PortNumber\"=dword:{p:08x}\r\n"));
        }
        s
    };
    let text = [
        "Windows Registry Editor Version 5.00\r\n".to_string(),
        session("Default%20Settings", "", Some("ssh"), Some(22)),
        session("web", "web.example", Some("ssh"), Some(2222)),
        session("old", "old.example", None, None),
        session("switch", "10.0.0.9", Some("telnet"), None),
        session("console", "COM1", Some("serial"), None),
        session("raw1", "r.example", Some("raw"), Some(7)),
        session("raw2", "r.example", Some("raw"), Some(8)),
        session("legacy", "l.example", Some("rlogin"), Some(513)),
    ]
    .concat();
    let hosts = parse_client_hosts(text.clone()).unwrap();
    let keys: Vec<&str> = hosts.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys, vec!["0", "1", "2"], "each row has a key of its own");
    let got: Vec<(&str, &str, u16, bool)> = hosts.iter().map(|h| (h.host_alias.as_str(), h.hostname.as_str(), h.port, h.telnet)).collect();
    assert_eq!(
        got,
        vec![("web", "web.example", 2222, false), ("old", "old.example", 22, false), ("switch", "10.0.0.9", 23, true)],
        "no Default Settings; a session with no Protocol is SSH; telnet on 23"
    );
    assert_eq!(
        parse_client_import(text).unwrap().skipped.as_deref(),
        Some("Not imported: 2 raw, 1 rlogin and 1 serial sessions — Submarine imports PuTTY's SSH and telnet sessions.")
    );
    let only_serial = ["Windows Registry Editor Version 5.00\r\n".to_string(), session("console", "COM1", Some("serial"), None)].concat();
    let err = parse_client_hosts(only_serial).err().expect("nothing to import");
    assert!(err.contains("1 serial session"), "{err}");
}

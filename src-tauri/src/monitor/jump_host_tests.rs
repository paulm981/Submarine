//! The fork's own tests, kept in a file of their own (#136) so a sync with
//! the original project doesn't conflict on them.

use super::*;
use crate::ssh_test_server::{serve, TestServer};
use std::sync::atomic::Ordering;

/// A TestServer on a loopback port, as monitoring connects over TCP.
async fn listen(server: TestServer) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            serve(server.clone(), |_| {}, socket);
        }
    });
    port
}

/// known_hosts trusting the test servers' key for `trusted` hosts only.
fn known_hosts(trusted: &[(&str, u16)]) -> Arc<std::sync::Mutex<Option<rusqlite::Connection>>> {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE known_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT, port INTEGER, fingerprint TEXT, key_type TEXT);")
        .unwrap();
    let fp = crate::ssh_manager::host_key_fingerprint(crate::ssh_test_server::host_key().public_key());
    for (host, port) in trusted {
        conn.execute(
            "INSERT INTO known_hosts (host, port, fingerprint, key_type) VALUES (?1, ?2, ?3, 'ssh-ed25519')",
            rusqlite::params![host, port, fp],
        )
        .unwrap();
    }
    Arc::new(std::sync::Mutex::new(Some(conn)))
}

fn node(host: &str, port: u16, jump: Option<NodeAuth>) -> NodeAuth {
    NodeAuth {
        host: host.into(),
        port,
        username: "test".into(),
        password: Some("test".into()),
        private_key: None,
        passphrase: None,
        proxy_type: "none".into(),
        proxy_host: None,
        proxy_port: None,
        jump: jump.map(Box::new),
        name: None,
        known_hosts: KnownHosts::default(),
    }
}

fn config(f: impl FnOnce(&mut crate::ssh_config::HostConfig)) -> Option<crate::ssh_config::HostConfig> {
    let mut c = crate::ssh_config::HostConfig::default();
    f(&mut c);
    Some(c)
}

/// The jump host as the config resolves it, like resolve_jump_host.
fn jump_target(
    saved: &NodeAuth,
    c: Option<crate::ssh_config::HostConfig>,
) -> Option<(crate::ConfigTarget, Option<crate::ssh_config::Proxy>)> {
    let mut t = crate::ConfigTarget::apply(&saved.host, saved.port, &saved.username, c);
    let proxy = t.proxy(None);
    Some((t, proxy))
}

// The monitor dials the jump host where the terminal does: an alias's
// HostName, Port and (for a blank username) User.
#[test]
fn a_jump_hosts_ssh_config_alias_is_followed() {
    let saved = node("bastion", 22, None);
    let mut auth = node("inner.example", 22, Some(saved));
    auth.jump.as_mut().unwrap().username = String::new();
    let target = crate::ConfigTarget::apply("inner.example", 22, "test", None);
    let jump = jump_target(
        &auth.jump.clone().unwrap(),
        config(|c| {
            c.hostname = Some("10.0.0.9".into());
            c.port = Some(2222);
            c.user = Some("hop".into());
        }),
    );
    let resolved = apply_ssh_config(&auth, target, jump).expect("checked");
    let j = resolved.jump.expect("still through the jump host");
    assert_eq!((j.host.as_str(), j.port, j.username.as_str()), ("10.0.0.9", 2222, "hop"));
    assert_eq!(j.password.as_deref(), Some("test"), "the jump host's own login is kept");
    assert_eq!((resolved.host.as_str(), resolved.port), ("inner.example", 22));
}

// A jump host the config reaches through a ProxyCommand or a ProxyJump
// isn't checked, as for the server itself: the terminal goes through
// them, and dialling the jump host straight would check another path.
#[test]
fn a_jump_host_behind_a_config_proxy_is_not_checked() {
    let auth = node("inner.example", 22, Some(node("bastion", 22, None)));
    let saved = auth.jump.clone().unwrap();
    let target = crate::ConfigTarget::apply("inner.example", 22, "test", None);
    let jump = jump_target(&saved, config(|c| c.proxy = Some(crate::ssh_config::Proxy::Command("nc %h %p".into()))));
    assert_eq!(apply_ssh_config(&auth, target, jump).err().as_deref(), Some(NOT_CHECKED_JUMP_PROXY_COMMAND));

    let target = crate::ConfigTarget::apply("inner.example", 22, "test", None);
    let jump = jump_target(
        &saved,
        config(|c| {
            c.hostname = Some("10.0.0.9".into());
            c.proxy = Some(crate::ssh_config::Proxy::Jump("outer".into()));
        }),
    );
    assert_eq!(apply_ssh_config(&auth, target, jump).err().as_deref(), Some(NOT_CHECKED_JUMP_PROXY_JUMP));
    // The server's own ProxyJump gives way to its saved jump host.
    let target = crate::ConfigTarget::apply(
        "inner.example",
        22,
        "test",
        config(|c| c.proxy = Some(crate::ssh_config::Proxy::Jump("outer".into()))),
    );
    let jump = jump_target(&saved, None);
    assert!(apply_ssh_config(&auth, target, jump).is_ok());
}

#[tokio::test]
async fn monitoring_reaches_a_server_through_its_jump_host() {
    let bastion = TestServer::default();
    let behind = Arc::clone(&bastion.jump_target_received);
    let port = listen(bastion).await;
    let db = known_hosts(&[("127.0.0.1", port), ("inner.example", 22)]);
    let conn = connect_for_monitor(&db, &node("inner.example", 22, Some(node("127.0.0.1", port, None))), Duration::from_secs(10), None)
        .await
        .expect("a monitor session through the jump host");
    assert!(conn.jump.is_some() && !conn.is_closed());
    // What the monitor sends lands on the server behind the jump host.
    let channel = conn.session.channel_open_session().await.unwrap();
    channel.data(&b"ping"[..]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while behind.load(Ordering::SeqCst) < 4 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the bytes must reach the server behind the jump host");
}

#[tokio::test]
async fn monitoring_refuses_channels_the_server_opens() {
    let server = TestServer::default();
    let back = Arc::clone(&server.opened_back);
    let port = listen(server).await;
    let db = known_hosts(&[("127.0.0.1", port)]);
    let conn = connect_for_monitor(&db, &node("127.0.0.1", port, None), Duration::from_secs(10), None)
        .await
        .expect("a monitor session");
    let channel = conn.session.channel_open_session().await.unwrap();
    channel.exec(true, "open-back").await.unwrap();
    let tries = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let tries = back.lock().unwrap().clone();
            if !tries.is_empty() {
                return tries;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the server's attempts must finish");
    assert_eq!(tries.len(), 7);
    let accepted: Vec<_> = tries.iter().filter(|(_, ok)| *ok).map(|(kind, _)| *kind).collect();
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
    assert!(!conn.is_closed(), "refusing them keeps the session up");
}

#[tokio::test]
async fn an_untrusted_server_behind_the_jump_host_is_refused() {
    let port = listen(TestServer::default()).await;
    let db = known_hosts(&[("127.0.0.1", port)]);
    let err = connect_for_monitor(&db, &node("inner.example", 22, Some(node("127.0.0.1", port, None))), Duration::from_secs(10), None)
        .await
        .err()
        .expect("an unknown host key behind the jump host must be refused");
    assert!(!err.starts_with("jump host"), "the server's own key failed, not the jump host's: {err}");
}

/// known_hosts marking the test servers' key @revoked, read as a
/// connection reads it.
async fn revoking_the_test_key(host: &str, port: u16) -> KnownHosts {
    use russh::keys::PublicKeyBase64;
    let key = crate::ssh_test_server::host_key();
    let k = key.public_key();
    let dir = std::env::temp_dir().join(format!("submarine-mon-kh-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("known_hosts");
    std::fs::write(&file, format!("@revoked * {} {}\n", k.algorithm(), k.public_key_base64())).unwrap();
    let trust = crate::known_hosts::for_host(Some(vec![file.to_string_lossy().into_owned()]), host, port).await;
    let _ = std::fs::remove_dir_all(&dir);
    KnownHosts(Some(Arc::new(trust.expect("read"))))
}

// A key the app trusts but known_hosts marks @revoked is refused by the
// monitor too, for the server and for its jump host.
#[tokio::test]
async fn monitoring_refuses_a_revoked_key() {
    let port = listen(TestServer::default()).await;
    let db = known_hosts(&[("127.0.0.1", port), ("inner.example", 22)]);
    let mut direct = node("127.0.0.1", port, None);
    assert!(connect_for_monitor(&db, &direct, Duration::from_secs(10), None).await.is_ok(), "trusted, not revoked");
    direct.known_hosts = revoking_the_test_key("127.0.0.1", port).await;
    assert!(connect_for_monitor(&db, &direct, Duration::from_secs(10), None).await.is_err(), "revoked");

    let mut jump = node("127.0.0.1", port, None);
    jump.known_hosts = revoking_the_test_key("127.0.0.1", port).await;
    let err = connect_for_monitor(&db, &node("inner.example", 22, Some(jump)), Duration::from_secs(10), None)
        .await
        .err()
        .expect("a revoked jump host key must be refused");
    assert!(err.starts_with("jump host 127.0.0.1"), "{err}");
}

/// A TestServer that also shows a host certificate for its key, signed
/// by `ca` for 127.0.0.1.
async fn listen_with_certificate(ca: &russh::keys::PrivateKey) -> u16 {
    use russh::keys::ssh_key::certificate::{Builder, CertType};
    let host_key = crate::ssh_test_server::host_key();
    let now = crate::trust_policy::now();
    let mut b = Builder::new_with_random_nonce(&mut rand::rng(), host_key.public_key(), now - 60, now + 3600).unwrap();
    b.cert_type(CertType::Host).unwrap();
    b.valid_principal("127.0.0.1").unwrap();
    let cert = b.sign(ca).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let cert = cert.clone();
            serve(TestServer::default(), move |c| c.certificates = vec![cert], socket);
        }
    });
    port
}

async fn known_hosts_text(text: String, host: &str, port: u16) -> KnownHosts {
    let dir = std::env::temp_dir().join(format!("submarine-mon-kh-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("known_hosts");
    std::fs::write(&file, text).unwrap();
    let trust = crate::known_hosts::for_host(Some(vec![file.to_string_lossy().into_owned()]), host, port).await;
    let _ = std::fs::remove_dir_all(&dir);
    KnownHosts(Some(Arc::new(trust.expect("read"))))
}

// A server showing a host certificate: checked once you've approved the
// key in it (it's on file), never otherwise; a revoked CA is refused even
// then, so it was the certificate that was checked.
#[tokio::test]
async fn monitoring_checks_a_certificate_whose_key_is_on_file() {
    use russh::keys::PublicKeyBase64;
    let ca = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    let ca_line = format!("{} {}", ca.public_key().algorithm(), ca.public_key().public_key_base64());
    let port = listen_with_certificate(&ca).await;
    let mut auth = node("127.0.0.1", port, None);
    auth.known_hosts = known_hosts_text(format!("@cert-authority 127.0.0.1 {ca_line}\n"), "127.0.0.1", port).await;

    let on_file = known_hosts(&[("127.0.0.1", port)]);
    assert!(connect_for_monitor(&on_file, &auth, Duration::from_secs(10), None).await.is_ok(), "the key in it is on file");
    let nothing = known_hosts(&[]);
    assert!(connect_for_monitor(&nothing, &auth, Duration::from_secs(10), None).await.is_err(), "nothing on file: not trusted");

    auth.known_hosts =
        known_hosts_text(format!("@cert-authority 127.0.0.1 {ca_line}\n@revoked * {ca_line}\n"), "127.0.0.1", port).await;
    assert!(connect_for_monitor(&on_file, &auth, Duration::from_secs(10), None).await.is_err(), "a revoked CA");
}

#[tokio::test]
async fn an_untrusted_jump_host_is_refused() {
    let port = listen(TestServer::default()).await;
    let db = known_hosts(&[("inner.example", 22)]);
    let err = connect_for_monitor(&db, &node("inner.example", 22, Some(node("127.0.0.1", port, None))), Duration::from_secs(10), None)
        .await
        .err()
        .expect("an unknown jump host key must be refused");
    assert!(err.starts_with("jump host 127.0.0.1"), "{err}");
}

//! The fork's rules for what `~/.ssh/known_hosts` and the key files of
//! `~/.ssh/config` may do (#43, after the original project's #170), kept out
//! of the original project's code, which calls in here from one-line hooks.
//!
//! - The app's own trusted keys (in the vault) win. known_hosts can only
//!   answer the first-time question, never the changed-key warning. A key
//!   marked `@revoked` there is always refused. If known_hosts lists other
//!   keys for the host, the changed-key warning shows them (D3).

use crate::known_hosts::{HostTrust, Verdict};
use crate::ssh_manager::{host_key_fingerprint, HostKeyVerdict};
use russh::keys::{Certificate, PublicKey};

/// What to do with the host key a server offered.
#[derive(Debug, PartialEq)]
pub(crate) enum HostKeyDecision {
    /// Trusted without asking. `save`: put it in the app's trusted keys.
    Trusted { why: String, save: bool },
    /// Refused without asking; the reason goes to the log and the error.
    Refused(String),
    /// The usual question: first time, or (`mismatch`) the changed-key
    /// warning with what was trusted or listed before. `note` goes to the log.
    Ask { mismatch: bool, prior: Vec<String>, note: Option<String> },
}

/// A plain host key: `on_file` is what the app's trusted keys say about it,
/// `known_hosts` what the user's known_hosts files say (None when the
/// ~/.ssh/config switch is off or there's no file).
pub(crate) fn decide_plain_key(on_file: HostKeyVerdict, known_hosts: Option<&HostTrust>, key: &PublicKey) -> HostKeyDecision {
    let listed = match known_hosts.map(|t| t.check_key(key)) {
        Some(Verdict::Refused(why)) => return HostKeyDecision::Refused(why),
        Some(Verdict::Trusted(why)) => Some(why),
        _ => None,
    };
    match on_file {
        HostKeyVerdict::Known => HostKeyDecision::Trusted {
            why: "Host fingerprint found in known_hosts database. Verified.".into(),
            save: false,
        },
        HostKeyVerdict::Changed { prior } => HostKeyDecision::Ask {
            mismatch: true,
            prior,
            note: listed.map(|_| {
                "~/.ssh/known_hosts lists this key, but the key this app trusts for the server is a different one: \
                 the app's warning stands."
                    .to_string()
            }),
        },
        HostKeyVerdict::New => {
            if let Some(why) = listed {
                return HostKeyDecision::Trusted { why, save: true };
            }
            let others: Vec<String> = known_hosts
                .map(|t| {
                    t.listed_keys()
                        .iter()
                        .map(|(k, file)| format!("{} ({}) in {}", host_key_fingerprint(k), k.algorithm(), file))
                        .collect()
                })
                .unwrap_or_default();
            if others.is_empty() {
                HostKeyDecision::Ask { mismatch: false, prior: Vec::new(), note: None }
            } else {
                HostKeyDecision::Ask {
                    mismatch: true,
                    prior: others,
                    note: Some("~/.ssh/known_hosts lists a different key for this server.".into()),
                }
            }
        }
    }
}

/// A host certificate at `now` (seconds since the epoch): trusted only when
/// an `@cert-authority` line covers the host and the certificate checks out
/// against it (known_hosts::HostTrust::check_certificate: a host certificate,
/// signed by that CA, valid now, issued for the name connected to, nothing
/// revoked). russh's key exchange separately checks that the server holds
/// the certificate's key. Never saved without asking.
///
/// The certificate is checked first: one that fails is refused, whatever
/// the app's trusted keys say, and no answer can override that. `on_file`
/// is what those keys say about the key inside the certificate: when keys
/// are on file for the server and it isn't one of them, it's the usual
/// changed-key warning, as for a plain key (D3; servers get reinstalled).
pub(crate) fn decide_certificate(on_file: HostKeyVerdict, known_hosts: Option<&HostTrust>, cert: &Certificate, now: u64) -> HostKeyDecision {
    let why = match known_hosts.map(|t| t.check_certificate(cert, now)) {
        Some(Verdict::Trusted(why)) => why,
        Some(Verdict::Refused(why)) => return HostKeyDecision::Refused(why),
        Some(Verdict::Undecided(_)) | None => {
            return HostKeyDecision::Refused(
                "the server presented a host certificate, and no @cert-authority line in your known_hosts covers this host".into(),
            )
        }
    };
    match on_file {
        HostKeyVerdict::Changed { prior } => HostKeyDecision::Ask {
            mismatch: true,
            prior,
            note: Some(format!(
                "The server showed a certificate signed by a CA your known_hosts trusts ({why}), \
                 but the key in it isn't the one this app trusts for the server."
            )),
        },
        HostKeyVerdict::Known | HostKeyVerdict::New => HostKeyDecision::Trusted { why, save: false },
    }
}

/// Whether a client certificate (CertificateFile, `<key>-cert.pub`) is
/// worth offering at `now`: one that has expired or isn't valid yet would
/// only be turned down. `Err` says why, for the log.
pub(crate) fn client_certificate_usable(cert: &Certificate, now: u64) -> Result<(), String> {
    if now < cert.valid_after() {
        return Err("isn't valid yet".into());
    }
    if now >= cert.valid_before() {
        return Err("has expired".into());
    }
    Ok(())
}

/// `client_certificate_usable` now, with a log line for one that isn't
/// (`path`: where it was read from). For `retain` (#43, D6).
pub(crate) fn offer_client_certificate(path: &str, cert: &Certificate, log: impl FnOnce(&str)) -> bool {
    match client_certificate_usable(cert, now()) {
        Ok(()) => true,
        Err(why) => {
            log(&format!("The certificate {path} {why}; skipped. Renew it (with tsh login, for Teleport) to use it."));
            false
        }
    }
}

/// Whether Monitoring can't check host:port because only a certificate
/// vouches for it: known_hosts trusts a CA for it and the app has no key on
/// file for it (a certificate is never saved there, D4).
pub(crate) fn known_only_by_certificate(known_hosts: Option<&HostTrust>, conn: &rusqlite::Connection, host: &str, port: u16) -> bool {
    known_hosts.is_some_and(|t| t.has_cas())
        && !conn
            .query_row("SELECT 1 FROM known_hosts WHERE host=?1 AND port=?2 LIMIT 1", rusqlite::params![host, port], |_| Ok(()))
            .is_ok()
}

/// After `HostKeyAlgorithms` from ~/.ssh/config (#170) has had its say, the
/// kinds of host key the app trusts for the host are still asked for first,
/// as without it (`prefer_known_host_keys`): a list that put another kind
/// first would make a server with several keys show one the app doesn't
/// know, and the changed-key warning would follow. A list that leaves the
/// trusted kind out entirely is logged.
pub(crate) fn keep_known_kinds_first(
    db: &std::sync::Arc<std::sync::Mutex<Option<rusqlite::Connection>>>,
    host: &str,
    port: u16,
    preferred: &mut russh::Preferred,
    log: &mut Vec<String>,
) {
    let families = match db.lock() {
        Ok(guard) => guard.as_ref().map(|c| crate::ssh_manager::known_host_families(c, host, port)).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    keep_kinds_first(preferred, &families, log);
}

fn keep_kinds_first(preferred: &mut russh::Preferred, families: &[String], log: &mut Vec<String>) {
    crate::prefer_known_host_keys(preferred, families);
    let asked: Vec<String> = preferred.key.iter().map(|a| crate::ssh_manager::key_type_family(a.as_str())).collect();
    let missing: Vec<&str> = families.iter().filter(|f| !asked.contains(f)).map(String::as_str).collect();
    if !missing.is_empty() {
        log.push(format!(
            "~/.ssh/config: HostKeyAlgorithms leaves out the kind of host key this app trusts for this server ({}); \
             another kind of key from it will get the changed-key warning.",
            missing.join(", ")
        ));
    }
}

/// Seconds since the epoch.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::ssh_key::certificate::{Builder, CertType};
    use russh::keys::{Algorithm, PrivateKey, PublicKeyBase64};

    fn key() -> PrivateKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
    }

    fn line(k: &PublicKey) -> String {
        format!("{} {}", k.algorithm(), k.public_key_base64())
    }

    /// What a known_hosts file of `text` says about host:port, read the way a
    /// connection reads it (known_hosts::for_host).
    async fn known_hosts(text: &str, host: &str, port: u16) -> HostTrust {
        let dir = std::env::temp_dir().join(format!("submarine-kh-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known_hosts");
        std::fs::write(&file, text).unwrap();
        let trust = crate::known_hosts::for_host(Some(vec![file.to_string_lossy().into_owned()]), host, port).await;
        let _ = std::fs::remove_dir_all(&dir);
        trust.expect("the file is read")
    }

    fn hashed(name: &str) -> String {
        use base64::Engine;
        use hmac::{Hmac, KeyInit, Mac};
        let salt = b"0123456789abcdefghij";
        let mut mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(salt).unwrap();
        mac.update(name.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD;
        format!("|1|{}|{}", b64.encode(salt), b64.encode(mac.finalize().into_bytes()))
    }

    fn trusted_and_saved(d: &HostKeyDecision) -> bool {
        matches!(d, HostKeyDecision::Trusted { save: true, .. })
    }

    #[tokio::test]
    async fn a_listed_key_answers_only_the_first_time_question() {
        let k = key();
        let pk = k.public_key();
        let l = line(pk);
        // Plain, hashed, [host]:port and negated entries, as OpenSSH reads them.
        for (text, host, port) in [
            (format!("db.example.com,10.0.0.5 {l}\n"), "db.example.com", 22),
            (format!("{} {l}\n", hashed("db.example.com")), "db.example.com", 22),
            (format!("{} {l}\n", hashed("[db.example.com]:2222")), "db.example.com", 2222),
            (format!("[db.example.com]:2222 {l}\n"), "db.example.com", 2222),
            (format!("*.example.com,!bad.example.com {l}\n"), "db.example.com", 22),
        ] {
            let t = known_hosts(&text, host, port).await;
            assert!(trusted_and_saved(&decide_plain_key(HostKeyVerdict::New, Some(&t), pk)), "{text}");
            // The app already trusts it: trusted, nothing to save.
            assert!(matches!(decide_plain_key(HostKeyVerdict::Known, Some(&t), pk), HostKeyDecision::Trusted { save: false, .. }));
        }
        // Not for another port, nor a negated name: the usual first-time question.
        let first_time = HostKeyDecision::Ask { mismatch: false, prior: vec![], note: None };
        let t = known_hosts(&format!("db.example.com {l}\n"), "db.example.com", 2222).await;
        assert_eq!(decide_plain_key(HostKeyVerdict::New, Some(&t), pk), first_time);
        let t = known_hosts(&format!("*.example.com,!bad.example.com {l}\n"), "bad.example.com", 22).await;
        assert_eq!(decide_plain_key(HostKeyVerdict::New, Some(&t), pk), first_time);
        // No known_hosts at all (the switch off): as before.
        assert_eq!(decide_plain_key(HostKeyVerdict::New, None, pk), first_time);
    }

    #[tokio::test]
    async fn a_changed_key_is_never_trusted_because_known_hosts_lists_it() {
        let k = key();
        let t = known_hosts(&format!("db.example.com {}\n", line(k.public_key())), "db.example.com", 22).await;
        let prior = vec!["OLDFP (ssh-ed25519)".to_string()];
        match decide_plain_key(HostKeyVerdict::Changed { prior: prior.clone() }, Some(&t), k.public_key()) {
            HostKeyDecision::Ask { mismatch: true, prior: p, note: Some(note) } => {
                assert_eq!(p, prior);
                assert!(note.contains("warning stands"), "{note}");
            }
            other => panic!("the changed-key warning, not {other:?}"),
        }
    }

    #[tokio::test]
    async fn other_keys_listed_bring_the_changed_key_warning() {
        let (listed, offered) = (key(), PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve: russh::keys::EcdsaCurve::NistP256 }).unwrap());
        let t = known_hosts(&format!("db.example.com {}\n", line(listed.public_key())), "db.example.com", 22).await;
        match decide_plain_key(HostKeyVerdict::New, Some(&t), offered.public_key()) {
            HostKeyDecision::Ask { mismatch: true, prior, .. } => {
                assert_eq!(prior.len(), 1);
                assert!(prior[0].starts_with(&host_key_fingerprint(listed.public_key())), "{prior:?}");
                assert!(prior[0].contains("ssh-ed25519") && prior[0].contains("known_hosts"), "{prior:?}");
            }
            other => panic!("the changed-key warning, not {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_revoked_key_is_refused_even_when_the_app_trusts_it() {
        let k = key();
        let l = line(k.public_key());
        let t = known_hosts(&format!("db.example.com {l}\n@revoked * {l}\n"), "db.example.com", 22).await;
        for on_file in [HostKeyVerdict::Known, HostKeyVerdict::New, HostKeyVerdict::Changed { prior: vec!["x".into()] }] {
            assert!(matches!(decide_plain_key(on_file, Some(&t), k.public_key()), HostKeyDecision::Refused(_)));
        }
    }

    #[tokio::test]
    async fn monitoring_checks_a_server_unless_only_a_certificate_vouches_for_it() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE known_hosts (id INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT, port INTEGER, fingerprint TEXT, key_type TEXT);")
            .unwrap();
        let ca = key();
        let with_ca = known_hosts(&format!("@cert-authority *.sublab.test {}\n", line(ca.public_key())), "node1.sublab.test", 3022).await;
        let without = known_hosts(&format!("other.example {}\n", line(key().public_key())), "node1.sublab.test", 3022).await;
        assert!(known_only_by_certificate(Some(&with_ca), &conn, "node1.sublab.test", 3022));
        assert!(!known_only_by_certificate(Some(&without), &conn, "node1.sublab.test", 3022), "no CA: checked as usual");
        assert!(!known_only_by_certificate(None, &conn, "node1.sublab.test", 3022));
        // A key on file (approved, or saved because known_hosts listed it): checked.
        conn.execute("INSERT INTO known_hosts (host, port, fingerprint, key_type) VALUES ('node1.sublab.test', 3022, 'fp', 'ssh-ed25519')", [])
            .unwrap();
        assert!(!known_only_by_certificate(Some(&with_ca), &conn, "node1.sublab.test", 3022));
    }

    const NOW: u64 = 1_800_000_000;

    fn cert(ca: &PrivateKey, principals: &[&str], valid: (u64, u64), kind: CertType) -> Certificate {
        let mut b = Builder::new_with_random_nonce(&mut rand::rng(), key().public_key(), valid.0, valid.1).unwrap();
        b.cert_type(kind).unwrap();
        for p in principals {
            b.valid_principal(*p).unwrap();
        }
        b.sign(ca).unwrap()
    }

    #[tokio::test]
    async fn host_certificates_are_checked_against_the_listed_ca() {
        let ca = key();
        let ca_line = format!("@cert-authority *.sublab.test {}\n", line(ca.public_key()));
        let t = known_hosts(&ca_line, "node1.sublab.test", 3022).await;
        let valid = (NOW - 3600, NOW + 3600);
        let good = cert(&ca, &["node1.sublab.test"], valid, CertType::Host);
        assert!(matches!(decide_certificate(HostKeyVerdict::New, Some(&t), &good, NOW), HostKeyDecision::Trusted { save: false, .. }));
        let refused = |c: &Certificate, t: &HostTrust| matches!(decide_certificate(HostKeyVerdict::New, Some(t), c, NOW), HostKeyDecision::Refused(_));
        assert!(refused(&cert(&ca, &["node2.sublab.test"], valid, CertType::Host), &t), "another name");
        assert!(refused(&cert(&ca, &["node1.sublab.test"], (NOW - 7200, NOW - 3600), CertType::Host), &t), "expired");
        assert!(refused(&cert(&ca, &["node1.sublab.test"], (NOW + 3600, NOW + 7200), CertType::Host), &t), "not yet valid");
        assert!(refused(&cert(&ca, &["node1.sublab.test"], valid, CertType::User), &t), "a user certificate");
        assert!(refused(&cert(&key(), &["node1.sublab.test"], valid, CertType::Host), &t), "another CA");
        let revoked = known_hosts(&format!("{ca_line}@revoked * {}\n", line(ca.public_key())), "node1.sublab.test", 3022).await;
        assert!(refused(&good, &revoked), "a revoked CA");
        // No CA for the host, or no known_hosts: refused.
        let elsewhere = known_hosts(&ca_line, "db.example.com", 22).await;
        assert!(refused(&good, &elsewhere));
        assert!(matches!(decide_certificate(HostKeyVerdict::New, None, &good, NOW), HostKeyDecision::Refused(_)));
        // The app trusts another key for the server: a good certificate gets
        // the changed-key warning, as a plain key would, with the old keys
        // and a line about the certificate; a bad one is refused all the same.
        let changed = || HostKeyVerdict::Changed { prior: vec!["OLDFP (ssh-ed25519)".into()] };
        match decide_certificate(changed(), Some(&t), &good, NOW) {
            HostKeyDecision::Ask { mismatch: true, prior, note: Some(note) } => {
                assert_eq!(prior, vec!["OLDFP (ssh-ed25519)".to_string()]);
                assert!(note.contains("certificate signed by a CA your known_hosts trusts"), "{note}");
            }
            other => panic!("the changed-key warning, not {other:?}"),
        }
        for bad in [
            cert(&ca, &["node2.sublab.test"], valid, CertType::Host),
            cert(&ca, &["node1.sublab.test"], (NOW - 7200, NOW - 3600), CertType::Host),
            cert(&key(), &["node1.sublab.test"], valid, CertType::Host),
        ] {
            assert!(matches!(decide_certificate(changed(), Some(&t), &bad, NOW), HostKeyDecision::Refused(_)));
        }
        assert!(matches!(decide_certificate(changed(), Some(&revoked), &good, NOW), HostKeyDecision::Refused(_)));
        assert!(matches!(decide_certificate(changed(), None, &good, NOW), HostKeyDecision::Refused(_)));
        assert!(matches!(decide_certificate(HostKeyVerdict::Known, Some(&t), &good, NOW), HostKeyDecision::Trusted { save: false, .. }));
    }

    #[test]
    fn a_client_certificate_outside_its_window_isnt_offered() {
        let ca = key();
        assert!(client_certificate_usable(&cert(&ca, &["u"], (NOW - 10, NOW + 10), CertType::User), NOW).is_ok());
        assert_eq!(client_certificate_usable(&cert(&ca, &["u"], (NOW - 20, NOW - 10), CertType::User), NOW).unwrap_err(), "has expired");
        assert_eq!(client_certificate_usable(&cert(&ca, &["u"], (NOW + 10, NOW + 20), CertType::User), NOW).unwrap_err(), "isn't valid yet");
        // Now: an expired one is skipped with a line saying so.
        let mut said = String::new();
        let expired = cert(&ca, &["u"], (1_000, 2_000), CertType::User);
        assert!(!offer_client_certificate("~/.tsh/u-cert.pub", &expired, |m| said = m.to_string()));
        assert!(said.contains("~/.tsh/u-cert.pub has expired; skipped"), "{said}");
        let current = cert(&ca, &["u"], (now() - 60, now() + 3600), CertType::User);
        assert!(offer_client_certificate("c", &current, |_| panic!("no line for a usable one")));
    }

    #[test]
    fn host_key_algorithms_from_the_config_keep_the_trusted_kind_first() {
        let order = |p: &russh::Preferred| p.key.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let on_file = vec!["ssh-ed25519".to_string()];
        // A list that puts RSA first: the trusted Ed25519 is still asked for first.
        let mut p = crate::ssh_preferred_algorithms();
        let mut log = Vec::new();
        crate::apply_host_key_algorithms(&mut p, false, Some("rsa-sha2-512,ssh-ed25519"), &mut log);
        assert_eq!(order(&p)[0], "rsa-sha2-512");
        keep_kinds_first(&mut p, &on_file, &mut log);
        assert_eq!(order(&p), vec!["ssh-ed25519", "rsa-sha2-512"]);
        assert!(log.is_empty(), "{log:?}");
        // A list without it: said in the log.
        let mut p = crate::ssh_preferred_algorithms();
        crate::apply_host_key_algorithms(&mut p, false, Some("rsa-sha2-512"), &mut log);
        keep_kinds_first(&mut p, &on_file, &mut log);
        assert!(log.iter().any(|l| l.contains("leaves out") && l.contains("ssh-ed25519")), "{log:?}");
        // Nothing on file: the config's order stands.
        let mut p = crate::ssh_preferred_algorithms();
        crate::apply_host_key_algorithms(&mut p, false, Some("^rsa-sha2-256"), &mut Vec::new());
        keep_kinds_first(&mut p, &[], &mut Vec::new());
        assert_eq!(order(&p)[0], "rsa-sha2-256");
    }

    /// A certificate re-encoded with its CA signature labelled `alg`, as a
    /// server could send it.
    fn with_signature_algorithm(c: &Certificate, alg: russh::keys::Algorithm) -> Certificate {
        use russh::keys::ssh_encoding::Encode;
        let bytes = c.to_bytes().unwrap();
        let old = c.signature().encode_vec().unwrap();
        let new = russh::keys::ssh_key::Signature::new(alg, c.signature().as_bytes().to_vec()).unwrap().encode_vec().unwrap();
        let mut out = bytes[..bytes.len() - old.len() - 4].to_vec();
        new.encode(&mut out).unwrap();
        Certificate::from_bytes(&out).unwrap()
    }

    #[tokio::test]
    async fn host_certificates_follow_openssh_more_closely() {
        use russh::keys::ssh_key::private::RsaKeypair;
        let host = "node1.sublab.test";
        let valid = (NOW - 3600, NOW + 3600);
        // Critical options: refused.
        let ca = key();
        let t = known_hosts(&format!("@cert-authority *.sublab.test {}\n", line(ca.public_key())), host, 22).await;
        let mut b = Builder::new_with_random_nonce(&mut rand::rng(), key().public_key(), valid.0, valid.1).unwrap();
        b.cert_type(CertType::Host).unwrap();
        b.valid_principal(host).unwrap();
        b.critical_option("force-command", "true").unwrap();
        match decide_certificate(HostKeyVerdict::New, Some(&t), &b.sign(&ca).unwrap(), NOW) {
            HostKeyDecision::Refused(why) => assert!(why.contains("critical options"), "{why}"),
            other => panic!("refused, not {other:?}"),
        }
        // An RSA CA: rsa-sha2-512 passes; the same certificate labelled ssh-rsa (SHA-1) doesn't.
        let rsa_ca = PrivateKey::from(RsaKeypair::random(&mut rand::rng(), 2048).unwrap());
        let t = known_hosts(&format!("@cert-authority *.sublab.test {}\n", line(rsa_ca.public_key())), host, 22).await;
        let good = cert(&rsa_ca, &[host], valid, CertType::Host);
        assert!(matches!(decide_certificate(HostKeyVerdict::New, Some(&t), &good, NOW), HostKeyDecision::Trusted { .. }));
        let sha1 = with_signature_algorithm(&good, russh::keys::Algorithm::Rsa { hash: None });
        match decide_certificate(HostKeyVerdict::New, Some(&t), &sha1, NOW) {
            HostKeyDecision::Refused(why) => assert!(why.contains("SHA-1"), "{why}"),
            other => panic!("refused, not {other:?}"),
        }
        // No hosts named: valid for any, and the log says so.
        let ed_t = known_hosts(&format!("@cert-authority *.sublab.test {}\n", line(ca.public_key())), host, 22).await;
        let mut b = Builder::new_with_random_nonce(&mut rand::rng(), key().public_key(), valid.0, valid.1).unwrap();
        b.cert_type(CertType::Host).unwrap();
        b.all_principals_valid().unwrap();
        match decide_certificate(HostKeyVerdict::New, Some(&ed_t), &b.sign(&ca).unwrap(), NOW) {
            HostKeyDecision::Trusted { why, .. } => assert!(why.contains("names no hosts, so it's valid for any"), "{why}"),
            other => panic!("trusted, not {other:?}"),
        }
    }

    // A known_hosts file with bytes that aren't UTF-8 is still read: its
    // @revoked line still counts. A folder where a file should be is skipped.
    #[tokio::test]
    async fn a_known_hosts_file_that_isnt_utf8_still_revokes() {
        let k = key();
        let l = line(k.public_key());
        let dir = std::env::temp_dir().join(format!("submarine-kh-bytes-{}", rand::random::<u64>()));
        std::fs::create_dir_all(dir.join("a-folder")).unwrap();
        let file = dir.join("known_hosts");
        let mut bytes = b"caf\xe9.example ssh-ed25519 AAAA\n".to_vec();
        bytes.extend_from_slice(format!("@revoked * {l}\ndb.example.com {l}\n").as_bytes());
        std::fs::write(&file, bytes).unwrap();
        let files = vec![dir.join("a-folder").to_string_lossy().into_owned(), file.to_string_lossy().into_owned()];
        let t = crate::known_hosts::for_host(Some(files), "db.example.com", 22).await.expect("read");
        assert!(matches!(decide_plain_key(HostKeyVerdict::Known, Some(&t), k.public_key()), HostKeyDecision::Refused(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

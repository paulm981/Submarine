//! The fork's own tests in `key_format_tests`, kept in a file of their own (#136) so
//! a sync with the original project doesn't conflict on them. A child of
//! `key_format_tests`, so they use its helpers as they are.

use super::*;

// An IdentityFile from ~/.ssh/config takes the same road as a saved key
// (#170, #43): read with read_small_file, then decoded by
// decode_private_key (in decode_identity_file), whose KDF cap refuses an
// absurd cost before any derivation runs, passphrase or not.
#[test]
fn an_identity_file_with_an_absurd_kdf_cost_is_refused() {
    use ssh_key::private::{Ed25519Keypair, Ed25519PrivateKey};
    let kp = ssh_key::PrivateKey::from(Ed25519Keypair::from(Ed25519PrivateKey::from_bytes(&[9u8; 32])));
    let pem = kp.encrypt(&mut rand::rng(), "pw").unwrap().to_openssh(ssh_key::LineEnding::LF).unwrap().to_string();
    let dir = std::env::temp_dir().join(format!("submarine-idfile-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("id_ed25519");
    std::fs::write(&path, patch_bcrypt_rounds(&pem, u32::MAX)).unwrap();
    let text = read_small_file(&path.to_string_lossy()).unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(decode_private_key(&text, Some("pw")), Err(KeyDecodeError::Malformed(_))));
    assert!(matches!(decode_private_key(&text, None), Err(KeyDecodeError::Malformed(_))), "refused before asking for a passphrase");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    // A file too large to be a key isn't read at all.
    std::fs::write(&path, vec![b'a'; (MAX_KEY_FILE_BYTES + 1) as usize]).unwrap();
    assert!(read_small_file(&path.to_string_lossy()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

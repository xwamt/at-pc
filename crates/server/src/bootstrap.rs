//! First-boot bootstrap for the root API key.
//!
//! Security model (see `docs/plans/2026-09-20-api-key-gate-design.md`):
//! - The key is generated from the OS CSPRNG (`getrandom`), never derived from
//!   timestamps, PIDs or counters.
//! - 32 random bytes of secret (256 bit) encoded as 43 chars of base64url
//!   (no padding) plus 5 random bytes of key id encoded as 8 chars of
//!   lowercase Crockford base32, yielding `atpc_sk_<id>_<secret>`.
//! - The plaintext key is shown exactly once (startup banner / stdout) and
//!   stored in `<data_dir>/bootstrap_key.txt` with 0600 permissions on Unix.
//! - On every subsequent start the existing file is read back (idempotent);
//!   no new key is generated and nothing is printed.

use std::io::Write;
use std::path::{Path, PathBuf};

/// File name of the bootstrap key inside the data directory.
pub const BOOTSTRAP_KEY_FILE: &str = "bootstrap_key.txt";

/// Key id + secret length constraints used by [`validate_api_key_format`].
const KEY_ID_LEN: usize = 8;
const KEY_SECRET_LEN: usize = 43;
const KEY_PREFIX: &str = "atpc_sk_";

/// Crockford base32 alphabet (lowercase): 0-9 and 22 letters, excluding I, L, O, U.
const CROCKFORD32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Outcome of [`ensure_bootstrap_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapOutcome {
    /// A new key was generated and written to disk.
    Generated(String),
    /// A previously written bootstrap file was read back.
    Existing(String),
    /// Bootstrap was skipped because KeyStore credentials already exist.
    Skipped,
}

impl BootstrapOutcome {
    pub fn key(&self) -> Option<&str> {
        match self {
            BootstrapOutcome::Generated(k) | BootstrapOutcome::Existing(k) => Some(k.as_str()),
            BootstrapOutcome::Skipped => None,
        }
    }
}

/// Encodes 5 bytes as 8 lowercase Crockford base32 characters.
fn crockford_base32_id(bytes: &[u8; 5]) -> String {
    let mut buf = [0u8; KEY_ID_LEN];
    let acc = u64::from_be_bytes([
        0,
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        0,
        0,
    ]);
    // 40 bits, take 5-bit groups from the most significant side.
    for (i, slot) in buf.iter_mut().enumerate() {
        let shift = 35 - (i as u32) * 5;
        let index = ((acc >> shift) & 0x1f) as usize;
        *slot = CROCKFORD32[index];
    }
    String::from_utf8(buf.to_vec()).expect("alphabet is ASCII")
}

/// Encodes 32 bytes as 43 chars of base64url without padding.
fn base64url_secret(bytes: &[u8; 32]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Generates a fresh `atpc_sk_<id>_<secret>` API key using the OS CSPRNG.
pub fn generate_api_key() -> Result<String, getrandom::Error> {
    let mut secret = [0u8; 32];
    let mut id_bytes = [0u8; 5];
    getrandom::getrandom(&mut secret)?;
    getrandom::getrandom(&mut id_bytes)?;
    Ok(format!(
        "{KEY_PREFIX}{}_{}",
        crockford_base32_id(&id_bytes),
        base64url_secret(&secret)
    ))
}

/// Strict format check: `atpc_sk_` + 8 lowercase Crockford chars + `_` + 43
/// base64url chars. Used by bootstrap read-back and (later) the key store.
pub fn validate_api_key_format(key: &str) -> bool {
    let rest = match key.strip_prefix(KEY_PREFIX) {
        Some(rest) => rest,
        None => return false,
    };
    let (id, secret) = match rest.split_once('_') {
        Some(pair) => pair,
        None => return false,
    };
    if id.len() != KEY_ID_LEN || secret.len() != KEY_SECRET_LEN {
        return false;
    }
    id.bytes().all(|b| CROCKFORD32.contains(&b))
        && secret.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Extracts the first `atpc_sk_...` line from free-form file content.
fn extract_key_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(KEY_PREFIX) && validate_api_key_format(line))
        .map(str::to_string)
}

/// Reads the bootstrap key file if it exists and parses the key out of it.
pub fn read_bootstrap_key(data_dir: &Path) -> Option<String> {
    let path = bootstrap_key_path(data_dir);
    let content = std::fs::read_to_string(path).ok()?;
    extract_key_line(&content)
}

/// Path of the bootstrap key file inside `data_dir`.
pub fn bootstrap_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(BOOTSTRAP_KEY_FILE)
}

fn render_key_file(path: &Path, key: &str) -> String {
    format!(
        "at-pc server root API key (first-boot bootstrap)\n\
         \n\
         \x20 {key}\n\
         \n\
         This file was created with owner-only (0600) permissions on Unix.\n\
         Record the key somewhere safe, then delete this file:\n\
         \x20 rm \"{}\"\n",
        path.display()
    )
}

fn write_key_file(path: &Path, key: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create data dir {}: {e}", parent.display()))?;
        }
        // create_new refuses to clobber a concurrently created file.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("create {}: {e}", path.display()))?;
        file.write_all(render_key_file(path, key).as_bytes())
            .map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create data dir {}: {e}", parent.display()))?;
        }
        // Windows has no portable 0600 equivalent; the file inherits the
        // directory ACL. The banner already urges deleting the file.
        std::fs::write(path, render_key_file(path, key))
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        tracing::warn!(
            "bootstrap key file {} relies on directory ACLs on this platform",
            path.display()
        );
    }
    Ok(())
}

/// Directory in which `KeyStore::load_or_create` looks for `bootstrap_key.txt`, given a
/// keys-file path.
///
/// The bootstrap writer and the keystore migrator have to agree on this, otherwise the key
/// is written to a directory the keystore never reads: the store comes up empty, the startup
/// banner advertises a root key that cannot authenticate, and a loopback listener treats the
/// empty store as "no credentials configured" and answers 200 to unauthenticated requests.
///
/// An empty or bare relative path resolves to the process CWD, mirroring the keystore.
pub fn keystore_sidecar_dir(keys_file: &Path) -> PathBuf {
    match keys_file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Ensures a bootstrap root key exists in `data_dir`.
///
/// - File already present and parseable: returns [`BootstrapOutcome::Existing`].
/// - No file (or an unparseable one):
///   - If `keys.json` already exists with active credentials: KeyStore is already initialized;
///     does NOT generate an orphan phantom key and returns [`BootstrapOutcome::Skipped`].
///   - Otherwise: generates a new key, writes it with 0600 permissions and returns [`BootstrapOutcome::Generated`].
pub fn ensure_bootstrap_key(data_dir: &Path) -> Result<BootstrapOutcome, String> {
    ensure_bootstrap_key_with_keys_file(data_dir, None)
}

/// Ensures a bootstrap root key exists in `data_dir`, checking an optional custom `keys_file` path.
pub fn ensure_bootstrap_key_with_keys_file(
    data_dir: &Path,
    keys_file: Option<&Path>,
) -> Result<BootstrapOutcome, String> {
    if let Some(key) = read_bootstrap_key(data_dir) {
        return Ok(BootstrapOutcome::Existing(key));
    }

    let keys_path = keys_file
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data_dir.join("keys.json"));

    if keys_path.exists() {
        // Fail closed on a key store that exists but is unusable.
        //
        // A bootstrap root key is only ever registered into `keys.json` on the
        // *absent*-file branch of `KeyStore::load_or_create`. So once `keys.json`
        // exists, any key minted here would never authenticate. Swallowing the load
        // error (the previous `if let Ok(store)`) therefore did two harmful things at
        // once: it printed a convincing "this is your new root key" banner for a key
        // that cannot log in, and it let the process boot with no usable credentials
        // at all — a state an operator cannot distinguish from a healthy gate.
        // Refusing to start is strictly safer.
        let store = crate::keystore::KeyStore::load_or_create(&keys_path).map_err(|e| {
            format!(
                "key store {} exists but could not be loaded: {e}. Refusing to generate a \
                 bootstrap key that would never be registered. Restore the file from backup, \
                 or delete it to re-initialize (this revokes all existing keys).",
                keys_path.display()
            )
        })?;
        if store.has_active_keys() {
            return Ok(BootstrapOutcome::Skipped);
        }
        return Err(format!(
            "key store {} exists but holds no active (unrevoked) key. Refusing to write a \
             bootstrap key that would never be registered. Restore the file from backup, \
             or delete it to re-initialize (this revokes all existing keys).",
            keys_path.display()
        ));
    }

    let key = generate_api_key().map_err(|e| format!("CSPRNG failure: {e}"))?;
    write_key_file(&bootstrap_key_path(data_dir), &key)?;
    Ok(BootstrapOutcome::Generated(key))
}

/// Prints the one-time root key banner to stdout (also lands in `docker logs`).
pub fn print_bootstrap_banner(key: &str) {
    let file = bootstrap_key_path(&crate::config::default_data_dir());
    print_bootstrap_banner_with_file(key, &file);
}

/// Prints the one-time root key banner with an explicit key file path.
pub fn print_bootstrap_banner_with_file(key: &str, file: &Path) {
    println!("================================================================");
    println!(" at-pc server first boot: root API key generated");
    println!("   {key}");
    println!(" Store it now. It is also written to:");
    println!("   {}", file.display());
    println!(" After recording it, delete that file:");
    #[cfg(unix)]
    println!("   rm \"{}\"", file.display());
    #[cfg(windows)]
    println!("   del \"{}\"", file.display());
    println!("================================================================");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_match_the_strict_format() {
        for _ in 0..32 {
            let key = generate_api_key().expect("CSPRNG");
            assert!(validate_api_key_format(&key), "bad key: {key}");
            assert!(key.starts_with("atpc_sk_"));
            assert_eq!(key.len(), KEY_PREFIX.len() + KEY_ID_LEN + 1 + KEY_SECRET_LEN);
        }
    }

    #[test]
    fn generated_keys_are_unique() {
        let a = generate_api_key().expect("CSPRNG");
        let b = generate_api_key().expect("CSPRNG");
        assert_ne!(a, b);
    }

    #[test]
    fn format_validation_rejects_malformed_keys() {
        assert!(!validate_api_key_format("atpc_sk_short_secret"));
        assert!(!validate_api_key_format("ATPC_SK_abcd2345_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(!validate_api_key_format(
            "atpc_sk_abcd2345_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        )); // 44 chars
        assert!(!validate_api_key_format(
            "atpc_sk_abcd234l_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        )); // 'l' not in Crockford
        assert!(!validate_api_key_format("atpc_sk_abcd2345_AAA AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(!validate_api_key_format(""));
    }

    #[test]
    fn bootstrap_is_idempotent_across_restarts() {
        let dir = std::env::temp_dir().join(format!("atpc-bootstrap-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let first = ensure_bootstrap_key(&dir).expect("bootstrap");
        let second = ensure_bootstrap_key(&dir).expect("bootstrap read-back");
        assert_eq!(first.key(), second.key(), "restart must reuse the existing key");
        match second {
            BootstrapOutcome::Existing(_) => {}
            BootstrapOutcome::Generated(_) => panic!("second call must not regenerate"),
            BootstrapOutcome::Skipped => panic!("second call with existing bootstrap file must not be skipped"),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(bootstrap_key_path(&dir)).expect("stat");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn key_file_content_parses_back() {
        let key = generate_api_key().expect("CSPRNG");
        let content = render_key_file(Path::new("/tmp/x/bootstrap_key.txt"), &key);
        assert_eq!(extract_key_line(&content).as_deref(), Some(key.as_str()));
        assert!(content.contains("rm \"/tmp/x/bootstrap_key.txt\""));
    }

    #[test]
    fn bootstrap_skips_generation_when_keystore_has_active_keys() {
        let dir = std::env::temp_dir().join(format!(
            "atpc-bootstrap-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");

        // 1. First boot generates key and writes bootstrap_key.txt
        let first = ensure_bootstrap_key(&dir).expect("bootstrap");
        assert!(matches!(first, BootstrapOutcome::Generated(_)));

        // 2. KeyStore loads and migrates bootstrap_key.txt into keys.json
        let keys_file = dir.join("keys.json");
        let store = crate::keystore::KeyStore::load_or_create(&keys_file).expect("keystore");
        assert!(store.has_active_keys());

        // 3. Operator deletes bootstrap_key.txt
        let bootstrap_file = bootstrap_key_path(&dir);
        assert!(bootstrap_file.exists());
        std::fs::remove_file(&bootstrap_file).expect("remove bootstrap file");

        // 4. Second boot: bootstrap_key.txt is missing, but keys.json has active keys
        let second = ensure_bootstrap_key(&dir).expect("second bootstrap");
        assert_eq!(second, BootstrapOutcome::Skipped);

        // Verify bootstrap_key.txt was NOT recreated!
        assert!(
            !bootstrap_file.exists(),
            "bootstrap_key.txt must not be recreated when keys.json exists"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "atpc-bootstrap-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// P1-24: an unparseable `keys.json` must abort bootstrap rather than mint a root key
    /// that the key store will never accept, which the startup banner would still advertise
    /// as valid while the server ran with authentication effectively disabled.
    #[test]
    fn bootstrap_refuses_to_generate_on_corrupt_keystore() {
        let dir = unique_dir("corrupt");
        std::fs::write(dir.join("keys.json"), "{\"keys\":[{\"id\":\"abc").expect("write corrupt");

        let err = ensure_bootstrap_key(&dir).expect_err("corrupt store must not be bypassed");
        assert!(
            err.contains("could not be loaded"),
            "error must name the unreadable store, got: {err}"
        );
        assert!(
            !bootstrap_key_path(&dir).exists(),
            "no phantom bootstrap_key.txt may be written over a corrupt store"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// P1-24: an existing store holding no active key (fresh `--no-bootstrap` run, or every
    /// key revoked) gets the same fail-closed treatment — the new key would never be
    /// registered into `keys.json`, so printing one is pure theatre.
    #[test]
    fn bootstrap_refuses_to_generate_when_store_has_no_active_key() {
        let dir = unique_dir("noactive");
        std::fs::write(dir.join("keys.json"), "{\"keys\":[]}").expect("write empty store");

        let err = ensure_bootstrap_key(&dir).expect_err("empty store must not be bypassed");
        assert!(
            err.contains("no active"),
            "error must explain the empty store, got: {err}"
        );
        assert!(!bootstrap_key_path(&dir).exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}

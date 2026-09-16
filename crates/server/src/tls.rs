//! TLS Configuration and Acceptor Setup for HTTPS / WSS.
//! Supports PEM certificates, private keys, and optional client CA for mTLS.

use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

use crate::config::ServerConfig;

/// Ensures the default crypto provider (ring) is installed for rustls
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Loads certificates and private key from PEM files and constructs an `Arc<rustls::ServerConfig>`
pub fn load_rustls_server_config(
    cert_path: &Path,
    key_path: &Path,
    client_ca_path: Option<&Path>,
) -> Result<Arc<rustls::ServerConfig>, String> {
    ensure_crypto_provider();

    let cert_file = File::open(cert_path)
        .map_err(|e| format!("Failed to open TLS cert file {:?}: {}", cert_path, e))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse TLS certificates: {}", e))?;

    if certs.is_empty() {
        return Err(format!("No certificates found in {:?}", cert_path));
    }

    let key_file = File::open(key_path)
        .map_err(|e| format!("Failed to open TLS private key file {:?}: {}", key_path, e))?;
    let mut key_reader = BufReader::new(key_file);
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| format!("Failed to parse TLS private key: {}", e))?
        .ok_or_else(|| format!("No private key found in {:?}", key_path))?;

    let builder = rustls::ServerConfig::builder();

    let server_config = if let Some(ca_path) = client_ca_path {
        let ca_file = File::open(ca_path)
            .map_err(|e| format!("Failed to open client CA file {:?}: {}", ca_path, e))?;
        let mut ca_reader = BufReader::new(ca_file);
        let mut root_store = rustls::RootCertStore::empty();
        for cert_res in rustls_pemfile::certs(&mut ca_reader) {
            let c = cert_res.map_err(|e| format!("Failed to parse client CA cert: {}", e))?;
            root_store
                .add(c)
                .map_err(|e| format!("Failed to add client CA to trust store: {}", e))?;
        }

        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| format!("Failed to build WebPkiClientVerifier: {}", e))?;

        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(certs, key)
            .map_err(|e| {
                format!(
                    "Failed to configure TLS server config with client verifier: {}",
                    e
                )
            })?
    } else {
        builder
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("Failed to configure TLS server config: {}", e))?
    };

    Ok(Arc::new(server_config))
}

/// Creates an optional TlsAcceptor from ServerConfig if TLS is enabled
pub fn create_tls_acceptor(config: &ServerConfig) -> Result<Option<TlsAcceptor>, String> {
    if let (Some(cert_path), Some(key_path)) = (&config.tls_cert_path, &config.tls_key_path) {
        let sc =
            load_rustls_server_config(cert_path, key_path, config.tls_client_ca_path.as_deref())?;
        Ok(Some(TlsAcceptor::from(sc)))
    } else {
        Ok(None)
    }
}

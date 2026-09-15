//! TLS Configuration and Connector setup for Agent WSS client.
//! Supports CA verification, mTLS client certificates, and insecure skip verify.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio_rustls::TlsConnector;

/// Ensures the default crypto provider (ring) is installed for rustls
pub fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Debug)]
struct NoopServerCertVerifier;

impl rustls::client::danger::ServerCertVerifier for NoopServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Builds an `Arc<rustls::ClientConfig>` based on CA, mTLS, and insecure options.
pub fn build_tls_client_config(
    ca_cert_path: Option<&Path>,
    client_cert_path: Option<&Path>,
    client_key_path: Option<&Path>,
    insecure_skip_verify: bool,
) -> Result<Arc<rustls::ClientConfig>, String> {
    ensure_crypto_provider();

    let builder = rustls::ClientConfig::builder();

    let config_builder = if insecure_skip_verify {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoopServerCertVerifier))
    } else {
        let mut root_store = rustls::RootCertStore::empty();
        if let Some(ca) = ca_cert_path {
            let ca_file = File::open(ca)
                .map_err(|e| format!("Failed to open CA cert {:?}: {}", ca, e))?;
            let mut ca_reader = BufReader::new(ca_file);
            for cert_res in rustls_pemfile::certs(&mut ca_reader) {
                let c = cert_res.map_err(|e| format!("Failed to parse CA cert: {}", e))?;
                root_store
                    .add(c)
                    .map_err(|e| format!("Failed to add CA cert: {}", e))?;
            }
        } else {
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }
        builder.with_root_certificates(root_store)
    };

    let client_config = if let (Some(cert_path), Some(key_path)) = (client_cert_path, client_key_path) {
        let cert_file = File::open(cert_path)
            .map_err(|e| format!("Failed to open client cert file {:?}: {}", cert_path, e))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to parse client certs: {}", e))?;

        let key_file = File::open(key_path)
            .map_err(|e| format!("Failed to open client key file {:?}: {}", key_path, e))?;
        let mut key_reader = BufReader::new(key_file);
        let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| format!("Failed to parse client private key: {}", e))?
            .ok_or_else(|| format!("No private key found in {:?}", key_path))?;

        config_builder
            .with_client_auth_cert(certs, key)
            .map_err(|e| format!("Failed to configure client certificate: {}", e))?
    } else {
        config_builder.with_no_client_auth()
    };

    Ok(Arc::new(client_config))
}

/// Creates a `TlsConnector` from the configured TLS options
pub fn create_tls_connector(
    ca_cert_path: Option<&Path>,
    client_cert_path: Option<&Path>,
    client_key_path: Option<&Path>,
    insecure_skip_verify: bool,
) -> Result<TlsConnector, String> {
    let client_config = build_tls_client_config(
        ca_cert_path,
        client_cert_path,
        client_key_path,
        insecure_skip_verify,
    )?;
    Ok(TlsConnector::from(client_config))
}

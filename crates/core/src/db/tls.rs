//! TLS for database connections, following libpq's `sslmode`,
//! `sslrootcert`, `sslcert` and `sslkey` semantics.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::profile::{ConnProfile, SslMode};
use crate::CoreError;

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accepts any certificate but still checks the handshake signatures — what
/// libpq does for `prefer`/`require`: encryption without authentication.
#[derive(Debug)]
struct EncryptOnly(Arc<CryptoProvider>);

impl ServerCertVerifier for EncryptOnly {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Checks the chain like `verify-full` but accepts any host name — libpq's
/// `verify-ca`. Cloud SQL server certificates name the instance, not the IP
/// address clients connect to.
#[derive(Debug)]
struct ChainOnly(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for ChainOnly {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
        {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

/// The certificate settings a client config depends on.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    mode: SslMode,
    root: Option<String>,
    cert: Option<String>,
    key: Option<String>,
}

fn file_error(what: &str, path: &str, e: impl std::fmt::Display) -> CoreError {
    CoreError::Config(format!("{what} {path}: {e}"))
}

/// `sslrootcert` when given, else the system trust store.
fn roots(path: Option<&str>) -> crate::Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    match path {
        Some(path) => {
            let certs = CertificateDer::pem_file_iter(path)
                .map_err(|e| file_error("sslrootcert", path, e))?;
            for cert in certs {
                let cert = cert.map_err(|e| file_error("sslrootcert", path, e))?;
                roots
                    .add(cert)
                    .map_err(|e| file_error("sslrootcert", path, e))?;
            }
            if roots.is_empty() {
                return Err(file_error("sslrootcert", path, "no certificates found"));
            }
        }
        None => {
            // Unreadable or malformed system certificates are skipped.
            for cert in rustls_native_certs::load_native_certs().certs {
                let _ = roots.add(cert);
            }
        }
    }
    Ok(roots)
}

fn build(k: &Key) -> crate::Result<ClientConfig> {
    let provider = provider();
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions");
    let builder = match k.mode {
        SslMode::VerifyFull => builder.with_root_certificates(roots(k.root.as_deref())?),
        SslMode::VerifyCa => {
            let verifier = WebPkiServerVerifier::builder_with_provider(
                Arc::new(roots(k.root.as_deref())?),
                provider,
            )
            .build()
            .map_err(|e| CoreError::Config(format!("TLS: {e}")))?;
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(ChainOnly(verifier)))
        }
        _ => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(EncryptOnly(provider))),
    };
    match (&k.cert, &k.key) {
        (Some(cert), Some(key)) => {
            let chain = CertificateDer::pem_file_iter(cert)
                .and_then(|certs| certs.collect::<std::result::Result<Vec<_>, _>>())
                .map_err(|e| file_error("sslcert", cert, e))?;
            let key_der =
                PrivateKeyDer::from_pem_file(key).map_err(|e| file_error("sslkey", key, e))?;
            builder
                .with_client_auth_cert(chain, key_der)
                .map_err(|e| CoreError::Config(format!("client certificate: {e}")))
        }
        (Some(_), None) | (None, Some(_)) => Err(CoreError::Config(
            "sslcert and sslkey must be given together.".into(),
        )),
        (None, None) => Ok(builder.with_no_client_auth()),
    }
}

/// The connector for `profile`'s TLS settings (configs are cached; loading
/// system roots is slow).
pub(super) fn connector(profile: &ConnProfile) -> crate::Result<MakeRustlsConnect> {
    static CONFIGS: Mutex<Option<HashMap<Key, ClientConfig>>> = Mutex::new(None);
    let key = Key {
        mode: profile.ssl,
        root: profile.ssl_root_cert.clone(),
        cert: profile.ssl_cert.clone(),
        key: profile.ssl_key.clone(),
    };
    let mut configs = CONFIGS.lock().unwrap_or_else(|e| e.into_inner());
    let configs = configs.get_or_insert_with(HashMap::new);
    let config = match configs.get(&key) {
        Some(c) => c.clone(),
        None => {
            let c = build(&key)?;
            configs.insert(key, c.clone());
            c
        }
    };
    Ok(MakeRustlsConnect::new(config))
}

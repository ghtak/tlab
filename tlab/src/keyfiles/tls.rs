use std::{net::IpAddr, path::PathBuf};

use anyhow::{Context, ensure};
use rcgen::{CertificateParams, ExtendedKeyUsagePurpose, KeyPair, KeyUsagePurpose};
use rustls::sign::CertifiedKey;
use serde::Deserialize;
use time::{Duration, OffsetDateTime};

use super::{
    files::{FilePair, validate_paths},
    pem::{parse_certificates, parse_private_key},
};

/// TLS certificate chain and matching, unencrypted PKCS#8 private key paths.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsCertFile {
    /// PEM certificates, with the end-entity certificate first.
    pub certificate_chain: PathBuf,
    pub private_key: PathBuf,
    #[serde(default)]
    pub generate_if_missing: bool,
    /// Used only when both files are absent and generation is enabled.
    #[serde(default)]
    pub generation: Option<TlsGenerationConfig>,
}

/// Parameters for a development-only P-256 self-signed server certificate.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsGenerationConfig {
    /// ASCII DNS names or IP addresses. Wildcards are not supported.
    #[serde(default = "default_subject_alt_names")]
    pub subject_alt_names: Vec<String>,
    #[serde(default = "default_validity_days")]
    pub validity_days: u32,
}

impl Default for TlsGenerationConfig {
    fn default() -> Self {
        Self {
            subject_alt_names: default_subject_alt_names(),
            validity_days: default_validity_days(),
        }
    }
}

/// A validated TLS PEM snapshot. No trust, expiry, or hostname policy is applied.
pub struct TlsMaterial {
    certificate_chain: String,
    private_key: String,
}

impl TlsMaterial {
    fn from_pem(private_key: String, certificate_chain: String) -> anyhow::Result<Self> {
        let chain = parse_certificates(&certificate_chain)?;
        let key = parse_private_key(&private_key)?;
        let provider = rustls::crypto::ring::default_provider();
        let certified = CertifiedKey::from_der(chain, key.into(), &provider)
            .context("failed to parse or match TLS certificate and private key")?;
        certified
            .keys_match()
            .context("TLS certificate and private key do not match")?;
        Ok(Self {
            private_key,
            certificate_chain,
        })
    }

    pub fn certificate_chain_pem(&self) -> &str {
        &self.certificate_chain
    }

    pub fn private_key_pem(&self) -> &str {
        &self.private_key
    }
}

impl TlsCertFile {
    /// Reads both files, or generates them only if both are absent and allowed.
    ///
    /// Partial, malformed, mismatched, or unreadable files produce an error and
    /// are not repaired. Generation publishes the private key first without
    /// overwriting any file; a later failure may leave that private key behind.
    /// Resolve partial files manually before retrying. Relative paths use the
    /// current working directory. Unix private keys are created with mode 0600;
    /// this module does not configure Windows ACLs.
    pub fn load(&self) -> crate::Result<TlsMaterial> {
        validate_paths(&self.private_key, &self.certificate_chain)?;
        self.load_inner()
            .with_context(|| {
                format!(
                    "failed to prepare TLS files `{}` and `{}`",
                    self.private_key.display(),
                    self.certificate_chain.display()
                )
            })
            .map_err(crate::Error::Internal)
    }

    fn load_inner(&self) -> anyhow::Result<TlsMaterial> {
        let files = FilePair::new(&self.private_key, &self.certificate_chain);
        match files.read_existing(self.generate_if_missing)? {
            Some((private_key, certificate_chain)) => {
                TlsMaterial::from_pem(private_key, certificate_chain)
            }
            None => {
                let material = generate_tls(self.generation.as_ref())?;
                files.persist(&material.private_key, &material.certificate_chain)?;
                Ok(material)
            }
        }
    }
}

fn default_subject_alt_names() -> Vec<String> {
    ["localhost", "127.0.0.1", "::1"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn default_validity_days() -> u32 {
    30
}

fn generate_tls(config: Option<&TlsGenerationConfig>) -> anyhow::Result<TlsMaterial> {
    let defaults = TlsGenerationConfig::default();
    let config = config.unwrap_or(&defaults);
    ensure!(
        !config.subject_alt_names.is_empty(),
        "TLS SAN list is empty"
    );
    for name in &config.subject_alt_names {
        ensure!(
            valid_subject_alt_name(name),
            "invalid TLS DNS name or IP address"
        );
    }
    ensure!(
        config.validity_days > 0,
        "TLS validity_days must be positive"
    );
    let now = OffsetDateTime::now_utc();
    let not_after = now
        .checked_add(Duration::days(i64::from(config.validity_days)))
        .context("TLS validity_days exceeds the supported date range")?;
    let mut params = CertificateParams::new(config.subject_alt_names.clone())
        .context("failed to construct TLS certificate parameters")?;
    params.not_before = now - Duration::minutes(5);
    params.not_after = not_after;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let keypair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .context("failed to generate TLS private key")?;
    let certificate = params
        .self_signed(&keypair)
        .context("failed to generate TLS certificate")?;
    TlsMaterial::from_pem(keypair.serialize_pem(), certificate.pem())
}

fn valid_subject_alt_name(name: &str) -> bool {
    if name.parse::<IpAddr>().is_ok() {
        return true;
    }
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

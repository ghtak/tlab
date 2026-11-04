use std::path::PathBuf;

use anyhow::{Context, ensure};
use rcgen::KeyPair;
use serde::Deserialize;

use super::{
    files::{FilePair, validate_paths},
    pem::{parse_private_key, parse_public_key},
};

/// Unencrypted PKCS#8 private key and matching SPKI public key PEM paths.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ed25519KeyFile {
    pub private_key: PathBuf,
    pub public_key: PathBuf,
    #[serde(default)]
    pub generate_if_missing: bool,
}

/// A validated Ed25519 private/public key PEM snapshot.
pub struct Ed25519Material {
    private_key: String,
    public_key: String,
}

impl Ed25519Material {
    fn from_pem(private_key: String, public_key: String) -> anyhow::Result<Self> {
        let key = parse_private_key(&private_key)?;
        let pair = KeyPair::from_pkcs8_der_and_sign_algo(&key, &rcgen::PKCS_ED25519)
            .context("failed to parse Ed25519 private key")?;
        ensure!(
            parse_public_key(&public_key)? == parse_public_key(&pair.public_key_pem())?,
            "Ed25519 public and private keys do not match"
        );
        Ok(Self {
            private_key,
            public_key,
        })
    }

    pub fn private_key_pem(&self) -> &str {
        &self.private_key
    }

    pub fn public_key_pem(&self) -> &str {
        &self.public_key
    }
}

impl Ed25519KeyFile {
    /// Reads and checks a key pair, or creates both files when absent and allowed.
    ///
    /// Uses the same preservation, partial-failure, path, and permission rules
    /// as [`TlsCertFile::load`](super::TlsCertFile::load). Public keys are read from the configured file;
    /// they are not silently replaced with a derived key.
    pub fn load(&self) -> crate::Result<Ed25519Material> {
        validate_paths(&self.private_key, &self.public_key)?;
        self.load_inner()
            .with_context(|| {
                format!(
                    "failed to prepare Ed25519 files `{}` and `{}`",
                    self.private_key.display(),
                    self.public_key.display()
                )
            })
            .map_err(crate::Error::Internal)
    }

    fn load_inner(&self) -> anyhow::Result<Ed25519Material> {
        let files = FilePair::new(&self.private_key, &self.public_key);
        match files.read_existing(self.generate_if_missing)? {
            Some((private_key, public_key)) => Ed25519Material::from_pem(private_key, public_key),
            None => {
                let material = generate_ed25519()?;
                files.persist(&material.private_key, &material.public_key)?;
                Ok(material)
            }
        }
    }
}

fn generate_ed25519() -> anyhow::Result<Ed25519Material> {
    let keypair = KeyPair::generate_for(&rcgen::PKCS_ED25519)
        .context("failed to generate Ed25519 key pair")?;
    Ed25519Material::from_pem(keypair.serialize_pem(), keypair.public_key_pem())
}

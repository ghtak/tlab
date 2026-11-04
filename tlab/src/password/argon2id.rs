//! Argon2id hash generation and verification of supported PHC hashes.

use crate::{Error, Result, password::PasswordHasher};
use argon2::password_hash::{
    Error as HashError, PasswordHash, PasswordHasher as _, PasswordVerifier as _,
};

/// Settings for newly generated Argon2id hashes.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            memory_kib: 19_456,
            iterations: 2,
            parallelism: 1,
        }
    }
}

/// Generates and verifies Argon2id hashes.
pub struct Hasher {
    params: argon2::Params,
}
impl Hasher {
    pub fn new(config: &Config) -> Result<Self> {
        // OWASP
        if config.memory_kib < 19_456 {
            return Err(Error::InvalidInput(
                "Argon2id memory_kib must be at least 19456".into(),
            ));
        }
        // OWASP
        if config.iterations < 2 {
            return Err(Error::InvalidInput(
                "Argon2id iterations must be at least 2".into(),
            ));
        }
        let params = argon2::Params::new(
            config.memory_kib,
            config.iterations,
            config.parallelism,
            Some(32),
        )
        .map_err(|error| {
            Error::InvalidInput(format!("invalid Argon2id configuration: {error}").into())
        })?;
        Ok(Self { params })
    }

    fn argon2(&self) -> argon2::Argon2<'_> {
        argon2::Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            self.params.clone(),
        )
    }
}

impl PasswordHasher for Hasher {
    fn hash(&self, password: &str) -> Result<String> {
        let salt = crate::password::salt()?;
        self.argon2()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|error| {
                Error::Internal(anyhow::anyhow!("Argon2id hash generation failed: {error}"))
            })
    }
    fn verify(&self, password: &str, encoded_hash: &str) -> Result<bool> {
        let hash = PasswordHash::new(encoded_hash).map_err(|error| {
            Error::InvalidInput(format!("invalid PHC password hash: {error}").into())
        })?;
        let problem = if hash.algorithm.as_str() != "argon2id" {
            Some("expected an Argon2id password hash")
        } else if hash.version != Some(19) {
            Some("Argon2id password hash must use version 19")
        } else if hash.salt.is_none() || hash.hash.is_none() {
            Some("Argon2id password hash requires salt and digest")
        } else if hash.params.iter().count() != 3
            || ["m", "t", "p"]
                .iter()
                .any(|name| hash.params.get_decimal(*name).is_none())
        {
            Some("Argon2id password hash requires exactly one integer m, t and p parameter")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(Error::InvalidInput(problem.into()));
        }
        match self.argon2().verify_password(password.as_bytes(), &hash) {
            Ok(()) => Ok(true),
            Err(HashError::Password) => Ok(false),
            Err(error) => Err(Error::InvalidInput(
                format!("invalid Argon2id password hash: {error}").into(),
            )),
        }
    }
}

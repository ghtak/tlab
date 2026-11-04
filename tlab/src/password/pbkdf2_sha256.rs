//! PBKDF2-HMAC-SHA256 hash generation and verification of supported PHC hashes.

use crate::{Error, Result, password::{self, PasswordHasher}};
use argon2::password_hash::{
    Error as HashError, PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString,
};

/// Settings for newly generated PBKDF2-SHA256 hashes.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub iterations: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            iterations: 600_000,
        }
    }
}

/// Generates and verifies PBKDF2-SHA256 hashes.
pub struct Hasher {
    iterations: u32,
}
impl Hasher {
    pub fn new(config: &Config) -> Result<Self> {
        // OWASP
        if config.iterations < 600_000 {
            return Err(Error::InvalidInput(
                "PBKDF2-SHA256 iterations must be at least 600000".into(),
            ));
        }
        Ok(Self {
            iterations: config.iterations,
        })
    }
}

impl PasswordHasher for Hasher {
    fn hash(&self, password: &str) -> Result<String> {
        let salt = crate::password::salt()?;
        pbkdf2::Pbkdf2
            .hash_password_customized(
                password.as_bytes(),
                None,
                None,
                pbkdf2::Params {
                    rounds: self.iterations,
                    output_length: 32,
                },
                &salt,
            )
            .map(|hash| hash.to_string())
            .map_err(|error| {
                Error::Internal(anyhow::anyhow!(
                    "PBKDF2-SHA256 hash generation failed: {error}"
                ))
            })
    }
    fn verify(&self, password: &str, encoded_hash: &str) -> Result<bool> {
        let hash = PasswordHash::new(encoded_hash).map_err(|error| {
            Error::InvalidInput(format!("invalid PHC password hash: {error}").into())
        })?;
        let problem = if hash.algorithm.as_str() != "pbkdf2-sha256" {
            Some("expected a PBKDF2-SHA256 password hash")
        } else if hash.version.is_some() {
            Some("PBKDF2-SHA256 password hash must not include a version")
        } else if hash.salt.is_none() || hash.hash.is_none() {
            Some("PBKDF2-SHA256 password hash requires salt and digest")
        } else if hash.params.iter().count() != 2
            || hash.params.get_decimal("i").is_none_or(|v| v == 0)
            || hash.params.get_decimal("l").is_none()
        {
            Some(
                "PBKDF2-SHA256 password hash requires exactly one positive integer i and one integer l parameter",
            )
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(Error::InvalidInput(problem.into()));
        }
        match pbkdf2::Pbkdf2.verify_password(password.as_bytes(), &hash) {
            Ok(()) => Ok(true),
            Err(HashError::Password) => Ok(false),
            Err(error) => Err(Error::InvalidInput(
                format!("invalid PBKDF2-SHA256 password hash: {error}").into(),
            )),
        }
    }
}

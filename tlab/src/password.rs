//! Password hashing with algorithm-specific configuration.
//!
//! Operations are synchronous and intentionally expensive. Async applications
//! should use a blocking executor and limit concurrent work. Verification uses
//! the costs stored in the hash; callers must control access to stored hashes.

use argon2::password_hash::SaltString;

use crate::{Error, Result};

pub mod argon2id;
pub mod pbkdf2_sha256;

/// Generates and verifies hashes using the implementation's algorithm.
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: &str) -> Result<String>;
    fn verify(&self, password: &str, encoded_hash: &str) -> Result<bool>;
}

pub fn salt() -> crate::Result<SaltString> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        Error::Internal(anyhow::anyhow!("failed to generate password salt: {error}"))
    })?;
    let salt = SaltString::encode_b64(&bytes).map_err(|error| {
        Error::Internal(anyhow::anyhow!("failed to encode password salt: {error}"))
    })?;
    Ok(salt)
}

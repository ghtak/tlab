//! Shared token primitives. Callers own trusted user data, session IDs, clocks,
//! Refresh persistence and its fixed seven-day expiry; no rotation happens here.
#![allow(
    dead_code,
    reason = "Token primitives are connected to login and HTTP in a later slice"
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tlab::keyfiles::Ed25519Material;
use uuid::Uuid;

pub const ACCESS_TTL_SECONDS: u64 = 600;

/// Errors deliberately contain no token, claim values, PEM or parser sources.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid token configuration or key")]
    InvalidConfiguration,
    #[error("invalid token input or timestamp")]
    InvalidInput,
    #[error("access token rejected")]
    InvalidAccess,
    #[error("access token signing failed")]
    SigningFailed,
    #[error("secure random generation failed")]
    RandomFailed,
}

// No Debug: claims may contain personal data. Map forces `user` to be an object.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessClaims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub sid: Uuid,
    pub iat: u64,
    pub exp: u64,
    pub user: Map<String, Value>,
}

impl AccessClaims {
    fn valid(&self, now: u64) -> bool {
        !self.sub.trim().is_empty()
            && self.sid.get_version_num() == 4
            && self.sid.get_variant() == uuid::Variant::RFC4122
            && self.iat <= now
            && now < self.exp
            && self.iat.checked_add(ACCESS_TTL_SECONDS) == Some(self.exp)
    }
}

/// Opaque secret; exposing its text must be intentional. No Debug or Serialize.
pub struct AccessToken(String);

impl AccessToken {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

pub struct AccessIssuer {
    key: EncodingKey,
    issuer: String,
    audience: String,
}

impl AccessIssuer {
    #[cfg(test)]
    pub(crate) fn with_invalid_signing_key() -> Self {
        Self {
            key: EncodingKey::from_ed_der(b"invalid key"),
            issuer: "test-issuer".into(),
            audience: "test-audience".into(),
        }
    }

    pub fn new(material: &Ed25519Material, issuer: &str, audience: &str) -> Result<Self, Error> {
        validate_config(issuer, audience)?;
        Ok(Self {
            key: EncodingKey::from_ed_pem(material.private_key_pem().as_bytes())
                .map_err(|_| Error::InvalidConfiguration)?,
            issuer: issuer.into(),
            audience: audience.into(),
        })
    }

    /// `now` is trusted Unix time in whole UTC seconds, supplied by the caller.
    /// `subject`, `session` and `user` must come from server-validated data.
    pub fn issue(
        &self,
        subject: &str,
        session: Uuid,
        user: Map<String, Value>,
        now: u64,
    ) -> Result<AccessToken, Error> {
        let claims = AccessClaims {
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            sub: subject.into(),
            sid: session,
            iat: now,
            exp: now
                .checked_add(ACCESS_TTL_SECONDS)
                .ok_or(Error::InvalidInput)?,
            user,
        };
        if !claims.valid(now) {
            return Err(Error::InvalidInput);
        }
        encode(&Header::new(Algorithm::EdDSA), &claims, &self.key)
            .map(AccessToken)
            .map_err(|_| Error::SigningFailed)
    }
}

/// Public-key-only verifier. The caller supplies a trusted SPKI Ed25519 PEM.
/// Token headers cannot select a key or algorithm.
#[derive(Clone)]
pub struct AccessVerifier {
    key: DecodingKey,
    validation: Validation,
}

impl AccessVerifier {
    pub fn new(public_key_pem: &str, issuer: &str, audience: &str) -> Result<Self, Error> {
        validate_config(issuer, audience)?;
        let key = DecodingKey::from_ed_pem(public_key_pem.as_bytes())
            .map_err(|_| Error::InvalidConfiguration)?;
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["iss", "aud", "sub", "exp"]);
        // jsonwebtoken uses the system clock. Keep signature/issuer/audience
        // checks enabled, but apply our strict time policy using supplied time.
        validation.validate_exp = false;
        validation.leeway = 0;
        Ok(Self { key, validation })
    }

    /// Returns claims only after verifying the signature and every policy check.
    /// No clock skew allowance: `now == exp` is already expired.
    pub fn verify(&self, token: &str, now: u64) -> Result<AccessClaims, Error> {
        let claims = decode::<AccessClaims>(token, &self.key, &self.validation)
            .map_err(|_| Error::InvalidAccess)?
            .claims;
        if !claims.valid(now) {
            return Err(Error::InvalidAccess);
        }
        Ok(claims)
    }
}

fn validate_config(issuer: &str, audience: &str) -> Result<(), Error> {
    if issuer.trim().is_empty() || audience.trim().is_empty() {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

/// 256 bits of OS randomness, encoded as 43 URL-safe unpadded characters.
/// No Debug/Serialize: raw Refresh values must never enter diagnostic output.
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn generate() -> Result<Self, Error> {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|_| Error::RandomFailed)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> RefreshHash {
        RefreshHash::of(&self.0)
    }
}

/// SHA-256 of the exact UTF-8 token text; suitable for lookup, never raw storage.
/// This type also omits Debug to keep token-derived material out of logs.
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshHash([u8; 32]);

impl RefreshHash {
    pub fn of(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests;

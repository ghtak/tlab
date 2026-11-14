//! Opaque refresh token generation and hashing.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("secure random generation failed")]
    RandomFailed,
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
mod tests {
    use super::*;

    #[test]
    fn refresh_is_random_url_safe_and_hashes_exact_token_text_repeatably() {
        let first = RefreshToken::generate().unwrap();
        let second = RefreshToken::generate().unwrap();
        assert_eq!(first.expose_secret().len(), 43);
        assert_eq!(
            URL_SAFE_NO_PAD.decode(first.expose_secret()).unwrap().len(),
            32
        );
        assert!(
            first
                .expose_secret()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert!(first.expose_secret() != second.expose_secret());
        assert!(first.hash() != second.hash());
        assert!(first.hash() == RefreshHash::of(first.expose_secret()));
        // Repeated lookups preserve the same hash: no rotation or consumption state.
        assert!(first.hash() == first.hash());
        assert!(RefreshHash::of("abc") != RefreshHash::of("abc "));
        // Independent published SHA-256 known-answer vector.
        assert_eq!(
            RefreshHash::of("abc").as_bytes(),
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }
}

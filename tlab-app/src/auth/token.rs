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
    #[error("access token expired")]
    ExpiredAccess,
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
        // Verify every non-expiry contract before classifying expiry. A bad
        // signature/issuer/audience/shape must never enable Refresh fallback.
        if !claims.valid(claims.iat) || claims.iat > now {
            return Err(Error::InvalidAccess);
        }
        if now >= claims.exp {
            return Err(Error::ExpiredAccess);
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
mod tests {
    use super::*;
    use serde_json::json;
    use tlab::keyfiles::Ed25519KeyFile;

    const NOW: u64 = 1_800_000_000;
    const ISSUER: &str = "https://auth.example.test";
    const AUDIENCE: &str = "tlab-api";
    const SID: &str = "5a9417e0-b843-46dc-86dc-e222d57ccc90";

    fn material() -> Ed25519Material {
        let dir = tempfile::tempdir().unwrap();
        Ed25519KeyFile {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: true,
        }
        .load()
        .unwrap()
    }

    fn user() -> Map<String, Value> {
        json!({"nickname": "tester", "roles": ["reader"]})
            .as_object()
            .unwrap()
            .clone()
    }

    fn claims() -> Value {
        json!({
            "iss": ISSUER, "aud": AUDIENCE, "sub": "42", "sid": SID,
            "iat": NOW, "exp": NOW + 600, "user": user()
        })
    }

    fn sign(material: &Ed25519Material, claims: &Value) -> String {
        encode(
            &Header::new(Algorithm::EdDSA),
            claims,
            &EncodingKey::from_ed_pem(material.private_key_pem().as_bytes()).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn real_keyfiles_sign_and_public_key_verifies_all_claims_and_expiry_boundary() {
        let material = material();
        let issuer = AccessIssuer::new(&material, ISSUER, AUDIENCE).unwrap();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        let token = issuer
            .issue("42", Uuid::parse_str(SID).unwrap(), user(), NOW)
            .unwrap();
        let decoded = verifier.verify(token.expose_secret(), NOW).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), claims());
        assert!(verifier.verify(token.expose_secret(), NOW + 599).is_ok());
        assert!(matches!(
            verifier.verify(token.expose_secret(), NOW + 600),
            Err(Error::ExpiredAccess)
        ));
        assert!(verifier.verify(token.expose_secret(), NOW + 601).is_err());
        assert!(verifier.verify(token.expose_secret(), NOW - 1).is_err());
        assert_eq!(
            jsonwebtoken::decode_header(token.expose_secret())
                .unwrap()
                .alg,
            Algorithm::EdDSA
        );
    }

    #[test]
    fn rejects_tampered_payload_signature_other_key_and_malformed_tokens() {
        let material = material();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        let token = sign(&material, &claims());
        let parts: Vec<_> = token.split('.').collect();
        let mut changed = claims();
        changed["sub"] = json!("999");
        let altered_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&changed).unwrap());
        let tampered = format!("{}.{}.{}", parts[0], altered_payload, parts[2]);
        assert!(verifier.verify(&tampered, NOW).is_err());
        let mut signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        signature[0] ^= 1;
        let tampered = format!(
            "{}.{}.{}",
            parts[0],
            parts[1],
            URL_SAFE_NO_PAD.encode(signature)
        );
        assert!(verifier.verify(&tampered, NOW).is_err());
        let other = self::material();
        assert!(verifier.verify(&sign(&other, &claims()), NOW).is_err());
        for invalid in [
            "",
            "not-a-jwt",
            "a.b.c",
            "...",
            &format!("{}.{}.", parts[0], parts[1]),
        ] {
            assert!(verifier.verify(invalid, NOW).is_err());
        }
    }

    #[test]
    fn expiry_classification_requires_signature_and_all_other_claim_contracts() {
        let material = material();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        let expired_now = NOW + 601;
        assert!(matches!(
            verifier.verify(&sign(&material, &claims()), expired_now),
            Err(Error::ExpiredAccess)
        ));
        let other_key = self::material();
        assert!(matches!(
            verifier.verify(&sign(&other_key, &claims()), expired_now),
            Err(Error::InvalidAccess)
        ));
        for (field, value) in [
            ("iss", json!("wrong")),
            ("aud", json!("wrong")),
            ("exp", json!(NOW + 599)),
            ("sub", json!("")),
            ("sid", json!(Uuid::nil())),
            ("user", json!("invalid")),
        ] {
            let mut invalid = claims();
            invalid[field] = value;
            assert!(
                matches!(
                    verifier.verify(&sign(&material, &invalid), expired_now),
                    Err(Error::InvalidAccess)
                ),
                "{field}"
            );
        }
    }

    #[test]
    fn rejects_algorithm_substitution_and_unsigned_tokens() {
        let material = material();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        // Even possession of the public PEM must not permit HMAC forgery.
        let hmac = encode(
            &Header::new(Algorithm::HS256),
            &claims(),
            &EncodingKey::from_secret(material.public_key_pem().as_bytes()),
        )
        .unwrap();
        assert!(verifier.verify(&hmac, NOW).is_err());
        let unsigned = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims()).unwrap())
        );
        assert!(verifier.verify(&unsigned, NOW).is_err());
    }

    #[test]
    fn rejects_missing_null_and_wrong_type_for_every_required_claim() {
        let material = material();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        for field in ["iss", "aud", "sub", "sid", "iat", "exp", "user"] {
            let mut missing = claims();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                verifier.verify(&sign(&material, &missing), NOW).is_err(),
                "missing {field}"
            );
            for value in [Value::Null, json!([]), json!(true)] {
                let mut invalid = claims();
                invalid[field] = value;
                assert!(
                    verifier.verify(&sign(&material, &invalid), NOW).is_err(),
                    "invalid type {field}"
                );
            }
        }
    }

    #[test]
    fn rejects_wrong_policy_values_in_correctly_signed_tokens() {
        let material = material();
        let verifier = AccessVerifier::new(material.public_key_pem(), ISSUER, AUDIENCE).unwrap();
        for (field, value) in [
            ("iss", json!("other-issuer")),
            ("aud", json!("other-api")),
            ("sub", json!(" ")),
            ("sid", json!("not-a-uuid")),
            ("sid", json!(Uuid::nil())),
            ("sid", json!("5a9417e0-b843-16dc-86dc-e222d57ccc90")),
            ("user", json!("{\"roles\":[\"admin\"]}")),
            ("iat", json!(NOW + 1)),
            ("iat", json!(-1)),
            ("exp", json!(NOW)),
            ("exp", json!(NOW + 601)),
            ("exp", json!(u64::MAX)),
            ("iat", json!(NOW as f64 + 0.5)),
        ] {
            let mut invalid = claims();
            invalid[field] = value;
            assert!(
                verifier.verify(&sign(&material, &invalid), NOW).is_err(),
                "invalid policy {field}"
            );
        }
    }

    #[test]
    fn rejects_invalid_configuration_keys_and_issuance_input_without_echoing_secrets() {
        let material = material();
        for (issuer, audience) in [(" ", AUDIENCE), (ISSUER, "")] {
            assert!(matches!(
                AccessIssuer::new(&material, issuer, audience),
                Err(Error::InvalidConfiguration)
            ));
            assert!(matches!(
                AccessVerifier::new(material.public_key_pem(), issuer, audience),
                Err(Error::InvalidConfiguration)
            ));
        }
        for key in ["sensitive-invalid-key", material.private_key_pem()] {
            let error = AccessVerifier::new(key, ISSUER, AUDIENCE).err().unwrap();
            assert_eq!(error, Error::InvalidConfiguration);
            assert!(!format!("{error:?}: {error}").contains(key));
        }
        let issuer = AccessIssuer::new(&material, ISSUER, AUDIENCE).unwrap();
        assert!(matches!(
            issuer.issue("", Uuid::new_v4(), user(), NOW),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            issuer.issue("42", Uuid::nil(), user(), NOW),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            issuer.issue("42", Uuid::new_v4(), user(), u64::MAX),
            Err(Error::InvalidInput)
        ));
    }

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

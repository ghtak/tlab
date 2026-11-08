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
        Err(Error::InvalidAccess)
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

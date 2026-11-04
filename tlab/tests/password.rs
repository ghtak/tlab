use tlab::password::{PasswordHasher, argon2id, pbkdf2_sha256};

fn argon() -> argon2id::Hasher {
    argon2id::Hasher::new(&serde_json::from_str("{}").unwrap()).unwrap()
}
fn pbkdf() -> pbkdf2_sha256::Hasher {
    pbkdf2_sha256::Hasher::new(&serde_json::from_str("{}").unwrap()).unwrap()
}

#[test]
fn algorithms_hash_and_verify_their_own_hashes() {
    let argon = argon();
    let pbkdf = pbkdf();
    for hasher in [&argon as &dyn PasswordHasher, &pbkdf] {
        let first = hasher.hash("암호").unwrap();
        let second = hasher.hash("암호").unwrap();
        assert_ne!(first, second);
        assert!(hasher.verify("암호", &first).unwrap());
        assert!(!hasher.verify("wrong", &first).unwrap());
        let other: &dyn PasswordHasher = if first.starts_with("$argon2id$") {
            &pbkdf
        } else {
            &argon
        };
        assert!(other.verify("암호", &first).is_err());
    }
}

#[test]
fn configurations_and_untrusted_hashes_are_checked() {
    let bad_argon = serde_json::from_str(r#"{"memory_kib":1}"#).unwrap();
    let bad_pbkdf = serde_json::from_str(r#"{"iterations":0}"#).unwrap();
    assert!(
        argon2id::Hasher::new(&bad_argon)
            .err()
            .unwrap()
            .to_string()
            .contains("memory_kib")
    );
    assert!(
        pbkdf2_sha256::Hasher::new(&bad_pbkdf)
            .err()
            .unwrap()
            .to_string()
            .contains("iterations")
    );
    assert!(serde_json::from_str::<argon2id::Config>(r#"{"unknown":1}"#).is_err());
    assert!(serde_json::from_str::<pbkdf2_sha256::Config>(r#"{"unknown":1}"#).is_err());
    let high_argon =
        serde_json::from_str(r#"{"memory_kib":262145,"iterations":11,"parallelism":5}"#).unwrap();
    let high_pbkdf = serde_json::from_str(r#"{"iterations":2000001}"#).unwrap();
    assert!(argon2id::Hasher::new(&high_argon).is_ok());
    assert!(pbkdf2_sha256::Hasher::new(&high_pbkdf).is_ok());
    let hasher = argon();
    assert!(
        hasher
            .verify("secret", "bad")
            .unwrap_err()
            .to_string()
            .contains("PHC")
    );
    assert!(
        hasher
            .verify(
                "secret",
                "$argon2id$v=16$m=8,t=1,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA"
            )
            .unwrap_err()
            .to_string()
            .contains("version 19")
    );
    for invalid in [
        "bad",
        "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$pbkdf2-sha256$i=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$argon2id$v=16$m=8,t=1,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ",
        "$argon2id$v=19$m=8,t=0,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$pbkdf2-sha256$i=0,l=16$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$argon2id$v=19$m=8,t=1,p=1,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
        "$pbkdf2-sha512$i=1,l=16$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA",
    ] {
        if invalid == "$argon2id$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$AAAAAAAAAAAAAAAAAAAAAA" {
            assert!(hasher.verify("x", invalid).is_ok());
        } else {
            for verifier in [&hasher as &dyn PasswordHasher, &pbkdf()] {
                let error = verifier
                    .verify("secret", invalid)
                    .expect_err(invalid)
                    .to_string();
                assert!(!error.contains("secret"));
                assert!(!error.contains(invalid));
            }
        }
    }
}

#[test]
fn known_pbkdf2_vector_and_long_salt() {
    use base64ct::{Base64Unpadded, Encoding};
    let hasher = pbkdf();
    // Published PBKDF2-HMAC-SHA256 vector with 4096 iterations.
    let mut output = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
        b"passwordPASSWORDpassword",
        b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
        4096,
        &mut output,
    );
    assert_eq!(
        output,
        [
            0x34, 0x8c, 0x89, 0xdb, 0xcb, 0xd3, 0x2b, 0x2f, 0x32, 0xd8, 0x14, 0xb8, 0x11, 0x6e,
            0x84, 0xcf, 0x2b, 0x17, 0x34, 0x7e, 0xbc, 0x18, 0x00, 0x18, 0x1c, 0x4e, 0x2a, 0x1f,
            0xb8, 0xdd, 0x53, 0xe1
        ]
    );
    let vector = format!(
        "$pbkdf2-sha256$i=4096,l=32${}${}",
        Base64Unpadded::encode_string(b"saltSALTsaltSALTsaltSALTsaltSALTsalt"),
        Base64Unpadded::encode_string(&output)
    );
    assert!(hasher.verify("passwordPASSWORDpassword", &vector).unwrap());
    let salt = [42u8; 48];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(b"password", &salt, 1, &mut output);
    let phc = format!(
        "$pbkdf2-sha256$i=1,l=32${}${}",
        Base64Unpadded::encode_string(&salt),
        Base64Unpadded::encode_string(&output)
    );
    assert!(hasher.verify("password", &phc).unwrap());
    let salt = [42u8; 49];
    let phc = format!(
        "$pbkdf2-sha256$i=1,l=32${}${}",
        Base64Unpadded::encode_string(&salt),
        Base64Unpadded::encode_string(&output)
    );
    assert!(hasher.verify("password", &phc).is_err());
}

#[test]
fn config_loader_supports_both_algorithms() {
    #[derive(serde::Deserialize)]
    struct AppConfig {
        argon: argon2id::Config,
        pbkdf: pbkdf2_sha256::Config,
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[argon]\n[pbkdf]\n").unwrap();
    let app: AppConfig = tlab::config::load(&path, "TLAB_PASSWORD_TEST_UNIQUE").unwrap();
    argon2id::Hasher::new(&app.argon).unwrap();
    pbkdf2_sha256::Hasher::new(&app.pbkdf).unwrap();
}

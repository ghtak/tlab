use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use rcgen::{KeyPair, SigningKey};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, SubjectPublicKeyInfoDer, pem::PemObject};
use serde::Deserialize;
use tlab::keyfiles::{Ed25519KeyFile, TlsCertFile, TlsGenerationConfig, TlsMaterial};

const ED_PRIVATE: &str = include_str!("fixtures/keyfiles/ed25519-private.pem");
const ED_PUBLIC: &str = include_str!("fixtures/keyfiles/ed25519-public.pem");
const TLS_PRIVATE: &str = include_str!("fixtures/keyfiles/tls-private.pem");
const TLS_CERT: &str = include_str!("fixtures/keyfiles/tls-cert.pem");

fn ed_files(root: &Path, generate: bool) -> Ed25519KeyFile {
    Ed25519KeyFile {
        private_key: root.join("private.pem"),
        public_key: root.join("public.pem"),
        generate_if_missing: generate,
    }
}

fn tls_files(root: &Path, generate: bool) -> TlsCertFile {
    TlsCertFile {
        private_key: root.join("private.pem"),
        certificate_chain: root.join("chain.pem"),
        generate_if_missing: generate,
        generation: None,
    }
}

fn write_pair(private_path: &Path, public_path: &Path, private: &str, public: &str) {
    fs::write(private_path, private).unwrap();
    fs::write(public_path, public).unwrap();
}

#[test]
fn settings_compose_in_consumers_and_reject_omissions_and_typos() {
    #[derive(Debug, Deserialize)]
    struct Consumer {
        tls: Option<TlsCertFile>,
        key: Ed25519KeyFile,
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.toml");
    fs::write(&path, "[tls]\ncertificate_chain = 'chain.pem'\nprivate_key = 'tls.pem'\n[key]\nprivate_key = 'private.pem'\npublic_key = 'public.pem'\n").unwrap();
    let config: Consumer = tlab::config::load(&path, "TLAB_KEYFILES_TEST").unwrap();
    let tls = config.tls.unwrap();
    assert!(!tls.generate_if_missing);
    assert!(tls.generation.is_none());
    assert!(!config.key.generate_if_missing);
    assert_eq!(config.key.public_key, Path::new("public.pem"));
    for value in [
        serde_json::json!({"private_key": "private.pem"}),
        serde_json::json!({"private_key": "private.pem", "public_key": "public.pem", "genrate_if_missing": true}),
    ] {
        assert!(serde_json::from_value::<Ed25519KeyFile>(value).is_err());
    }
    assert!(
        serde_json::from_value::<TlsCertFile>(serde_json::json!({
            "private_key": "private.pem"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<TlsGenerationConfig>(serde_json::json!({
            "validty_days": 1
        }))
        .is_err()
    );
    let generation: TlsGenerationConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(
        generation.subject_alt_names,
        ["localhost", "127.0.0.1", "::1"]
    );
    assert_eq!(generation.validity_days, 30);
}

#[test]
fn missing_files_do_not_create_directories_when_generation_is_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("absent");
    assert!(ed_files(&root, false).load().is_err());
    assert!(tls_files(&root, false).load().is_err());
    assert!(!root.exists());
}

#[test]
fn external_ed25519_v1_files_are_preserved_and_support_signatures() {
    let directory = tempfile::tempdir().unwrap();
    let config = ed_files(directory.path(), true);
    let public = ED_PUBLIC.replace('\n', "\r\n");
    write_pair(&config.private_key, &config.public_key, ED_PRIVATE, &public);
    let material = config.load().unwrap();
    assert_eq!(material.private_key_pem(), ED_PRIVATE);
    assert_eq!(material.public_key_pem(), public);
    assert_eq!(fs::read_to_string(&config.private_key).unwrap(), ED_PRIVATE);
    assert_eq!(fs::read_to_string(&config.public_key).unwrap(), public);
    check_ed_signature(material.private_key_pem(), material.public_key_pem());
}

fn check_ed_signature(private: &str, public: &str) {
    let pair = KeyPair::from_pkcs8_pem_and_sign_algo(private, &rcgen::PKCS_ED25519).unwrap();
    let signature = pair.sign(b"test message").unwrap();
    let public = SubjectPublicKeyInfoDer::from_pem_slice(public.as_bytes()).unwrap();
    // RFC 8410's Ed25519 SPKI has a fixed algorithm prefix and a 32-byte key.
    let der: &[u8] = public.as_ref();
    assert_eq!(
        &der[..12],
        &[0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 3, 0x21, 0]
    );
    let verifier = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &der[12..]);
    verifier.verify(b"test message", &signature).unwrap();
    assert!(verifier.verify(b"changed message", &signature).is_err());
}

#[test]
fn generated_ed25519_files_survive_reload_and_have_private_permissions() {
    let directory = tempfile::tempdir().unwrap();
    let config = ed_files(&directory.path().join("nested"), true);
    let first = config.load().unwrap();
    check_ed_signature(first.private_key_pem(), first.public_key_pem());
    assert_eq!(
        fs::read_to_string(&config.private_key).unwrap(),
        first.private_key_pem()
    );
    assert_eq!(
        fs::read_to_string(&config.public_key).unwrap(),
        first.public_key_pem()
    );
    let second = config.load().unwrap();
    assert_eq!(first.private_key_pem(), second.private_key_pem());
    assert_eq!(first.public_key_pem(), second.public_key_pem());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config.private_key)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn external_tls_ec_and_rsa_files_load_without_generation_settings() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = tls_files(directory.path(), true);
    // Generation-only values have no effect on an existing certificate.
    config.generation = Some(TlsGenerationConfig {
        subject_alt_names: vec![],
        validity_days: 0,
    });
    for (private, certificate) in [
        (TLS_PRIVATE, TLS_CERT),
        (
            include_str!("fixtures/keyfiles/rsa-private.pem"),
            include_str!("fixtures/keyfiles/rsa-cert.pem"),
        ),
    ] {
        write_pair(
            &config.private_key,
            &config.certificate_chain,
            private,
            certificate,
        );
        let material = config.load().unwrap();
        assert_eq!(material.private_key_pem(), private);
        assert_eq!(material.certificate_chain_pem(), certificate);
        assert_eq!(fs::read_to_string(&config.private_key).unwrap(), private);
    }
}

#[test]
fn tls_supports_p384_and_ed25519_identities_and_certificate_chains() {
    let directory = tempfile::tempdir().unwrap();
    let config = tls_files(directory.path(), false);
    for algorithm in [&rcgen::PKCS_ECDSA_P384_SHA384, &rcgen::PKCS_ED25519] {
        let pair = KeyPair::generate_for(algorithm).unwrap();
        let certificate = rcgen::CertificateParams::new(vec!["localhost".into()])
            .unwrap()
            .self_signed(&pair)
            .unwrap();
        let chain = format!("{}{}", certificate.pem(), TLS_CERT);
        write_pair(
            &config.private_key,
            &config.certificate_chain,
            &pair.serialize_pem(),
            &chain,
        );
        assert_eq!(config.load().unwrap().certificate_chain_pem(), chain);
    }
}

#[test]
fn generated_tls_material_works_for_default_and_custom_subject_names() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = tls_files(directory.path(), true);
    let material = config.load().unwrap();
    for name in ["localhost", "127.0.0.1", "::1"] {
        handshake(&material, name).unwrap();
    }
    assert!(handshake(&material, "other.example").is_err());
    assert_eq!(
        config.load().unwrap().private_key_pem(),
        material.private_key_pem()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config.private_key)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let custom = directory.path().join("custom");
    config.private_key = custom.join("key.pem");
    config.certificate_chain = custom.join("chain.pem");
    config.generation = Some(TlsGenerationConfig {
        subject_alt_names: vec!["custom.example".into()],
        validity_days: 1,
    });
    let material = config.load().unwrap();
    handshake(&material, "custom.example").unwrap();
    assert!(handshake(&material, "localhost").is_err());
}

fn handshake(material: &TlsMaterial, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    handshake_at(material, name, None)
}

#[derive(Debug)]
struct FixedTime(rustls::pki_types::UnixTime);

impl rustls::time_provider::TimeProvider for FixedTime {
    fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
        Some(self.0)
    }
}

fn handshake_at(
    material: &TlsMaterial,
    name: &str,
    now: Option<rustls::pki_types::UnixTime>,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let chain = CertificateDer::pem_slice_iter(material.certificate_chain_pem().as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(material.private_key_pem().as_bytes())?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(chain[0].clone())?;
    let server_config = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(chain, key)?;
    let mut client_config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    if let Some(now) = now {
        client_config.time_provider = Arc::new(FixedTime(now));
    }
    let mut client =
        rustls::ClientConnection::new(Arc::new(client_config), name.to_owned().try_into()?)?;
    let mut server = rustls::ServerConnection::new(Arc::new(server_config))?;
    for _ in 0..10 {
        let mut to_server = Vec::new();
        client.write_tls(&mut to_server)?;
        server.read_tls(&mut to_server.as_slice())?;
        server.process_new_packets()?;
        let mut to_client = Vec::new();
        server.write_tls(&mut to_client)?;
        client.read_tls(&mut to_client.as_slice())?;
        client.process_new_packets()?;
        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(());
        }
    }
    panic!("in-memory TLS handshake did not finish");
}

#[test]
fn invalid_generation_settings_fail_before_creating_files() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = tls_files(&directory.path().join("absent"), true);
    for settings in [
        TlsGenerationConfig {
            subject_alt_names: vec![],
            validity_days: 30,
        },
        TlsGenerationConfig {
            subject_alt_names: vec!["bad name".into()],
            validity_days: 30,
        },
        TlsGenerationConfig {
            subject_alt_names: vec!["*.example".into()],
            validity_days: 30,
        },
        TlsGenerationConfig {
            subject_alt_names: vec!["localhost".into()],
            validity_days: 0,
        },
        TlsGenerationConfig {
            subject_alt_names: vec!["localhost".into()],
            validity_days: u32::MAX,
        },
    ] {
        config.generation = Some(settings);
        assert!(config.load().is_err());
        assert!(!config.private_key.parent().unwrap().exists());
    }
}

#[test]
fn generated_tls_validity_matches_default_custom_and_backdated_windows() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = tls_files(directory.path(), true);
    let material = config.load().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let at = |seconds: i64| {
        let epoch_seconds = now.as_secs().checked_add_signed(seconds).unwrap();
        Some(rustls::pki_types::UnixTime::since_unix_epoch(
            std::time::Duration::from_secs(epoch_seconds),
        ))
    };
    handshake_at(&material, "localhost", at(-120)).unwrap();
    assert!(handshake_at(&material, "localhost", at(-600)).is_err());
    handshake_at(&material, "localhost", at(29 * 86400)).unwrap();
    assert!(handshake_at(&material, "localhost", at(31 * 86400)).is_err());

    let custom = directory.path().join("custom");
    config.private_key = custom.join("key.pem");
    config.certificate_chain = custom.join("cert.pem");
    config.generation = Some(TlsGenerationConfig {
        validity_days: 1,
        ..Default::default()
    });
    let material = config.load().unwrap();
    assert!(handshake_at(&material, "localhost", at(2 * 86400)).is_err());
    // Reading files does not impose the TLS consumer's time policy.
    assert!(config.load().is_ok());
}

#[test]
fn partial_pairs_are_not_repaired_in_either_direction() {
    for private_exists in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let ed = ed_files(directory.path(), true);
        let existing = if private_exists {
            &ed.private_key
        } else {
            &ed.public_key
        };
        fs::write(existing, "existing").unwrap();
        assert!(ed.load().is_err());
        assert_eq!(fs::read_to_string(existing).unwrap(), "existing");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);

        let directory = tempfile::tempdir().unwrap();
        let tls = tls_files(directory.path(), true);
        let existing = if private_exists {
            &tls.private_key
        } else {
            &tls.certificate_chain
        };
        fs::write(existing, "existing").unwrap();
        assert!(tls.load().is_err());
        assert_eq!(fs::read_to_string(existing).unwrap(), "existing");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn bad_ed25519_files_never_trigger_replacement_or_expose_pem_in_errors() {
    let directory = tempfile::tempdir().unwrap();
    let config = ed_files(directory.path(), true);
    let other = KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
    for (private, public) in [
        (ED_PRIVATE.to_owned(), other.public_key_pem()),
        (TLS_PRIVATE.to_owned(), ED_PUBLIC.to_owned()),
        (ED_PRIVATE.to_owned(), "not a PEM file".into()),
        ("not a PEM file".into(), ED_PUBLIC.into()),
        (
            "-----BEGIN SECRET-INPUT-----oops\n".into(),
            ED_PUBLIC.into(),
        ),
        (format!("{ED_PRIVATE}{ED_PRIVATE}"), ED_PUBLIC.into()),
        (ED_PRIVATE.into(), format!("{ED_PUBLIC}{ED_PUBLIC}")),
    ] {
        write_pair(&config.private_key, &config.public_key, &private, &public);
        let error = config.load().err().unwrap();
        let diagnostic = format!("{error:?}");
        assert!(!diagnostic.contains("-----BEGIN"));
        assert!(!diagnostic.contains("SECRET-INPUT"));
        assert!(diagnostic.contains(&config.private_key.display().to_string()));
        assert_eq!(fs::read_to_string(&config.private_key).unwrap(), private);
        assert_eq!(fs::read_to_string(&config.public_key).unwrap(), public);
    }
}

#[test]
fn bad_tls_files_are_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let config = tls_files(directory.path(), true);
    for (private, certificate) in [
        (ED_PRIVATE, TLS_CERT),
        ("bad private key", TLS_CERT),
        (TLS_PRIVATE, "bad certificate"),
        (ED_PUBLIC, TLS_CERT),
        (TLS_PRIVATE, ED_PUBLIC),
        (
            TLS_PRIVATE,
            "-----BEGIN CERTIFICATE-----\nYWJj\n-----END CERTIFICATE-----\n",
        ),
    ] {
        write_pair(
            &config.private_key,
            &config.certificate_chain,
            private,
            certificate,
        );
        assert!(config.load().is_err());
        assert_eq!(fs::read_to_string(&config.private_key).unwrap(), private);
        assert_eq!(
            fs::read_to_string(&config.certificate_chain).unwrap(),
            certificate
        );
    }
}

#[test]
fn empty_and_equivalent_paths_are_invalid_input() {
    let directory = tempfile::tempdir().unwrap();
    let mut ed = ed_files(directory.path(), true);
    ed.private_key = "".into();
    assert!(matches!(ed.load(), Err(tlab::Error::InvalidInput(_))));
    ed.private_key = directory.path().join("private.pem");
    ed.public_key = directory.path().join("unused/../private.pem");
    assert!(matches!(ed.load(), Err(tlab::Error::InvalidInput(_))));
    let mut tls = tls_files(directory.path(), true);
    tls.certificate_chain = tls.private_key.clone();
    assert!(matches!(tls.load(), Err(tlab::Error::InvalidInput(_))));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn symlink_aliases_and_dangling_symlinks_are_not_generation_targets() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let config = ed_files(directory.path(), true);
    fs::write(&config.private_key, ED_PRIVATE).unwrap();
    symlink(&config.private_key, &config.public_key).unwrap();
    assert!(matches!(config.load(), Err(tlab::Error::InvalidInput(_))));
    fs::remove_file(&config.private_key).unwrap();
    assert!(config.load().is_err());
    assert!(!config.private_key.exists());
    assert!(
        fs::symlink_metadata(&config.public_key)
            .unwrap()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn unreadable_private_key_is_not_replaced_or_chmodded() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let config = ed_files(directory.path(), true);
    write_pair(
        &config.private_key,
        &config.public_key,
        ED_PRIVATE,
        ED_PUBLIC,
    );
    fs::set_permissions(&config.private_key, fs::Permissions::from_mode(0o0)).unwrap();
    // A privileged test runner may still read mode-000 files.
    let readable = fs::read(&config.private_key).is_ok();
    let result = config.load();
    let mode = fs::metadata(&config.private_key)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    fs::set_permissions(&config.private_key, fs::Permissions::from_mode(0o600)).unwrap();
    if !readable {
        assert!(result.is_err());
    }
    assert_eq!(mode, 0);
    assert_eq!(fs::read_to_string(&config.private_key).unwrap(), ED_PRIVATE);
}

#[test]
fn concurrent_generation_never_overwrites_the_winning_pair() {
    let directory = tempfile::tempdir().unwrap();
    let config = Arc::new(ed_files(directory.path(), true));
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let config = config.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                config
                    .load()
                    .ok()
                    .map(|material| material.private_key_pem().to_owned())
            })
        })
        .collect();
    let successes: Vec<_> = handles
        .into_iter()
        .filter_map(|handle| handle.join().unwrap())
        .collect();
    assert!(!successes.is_empty());
    let material = config.load().unwrap();
    assert!(
        successes
            .iter()
            .all(|key| key == material.private_key_pem())
    );
    check_ed_signature(material.private_key_pem(), material.public_key_pem());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

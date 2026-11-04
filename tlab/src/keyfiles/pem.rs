use anyhow::{Context, bail, ensure};
use rustls::pki_types::{
    CertificateDer, PrivatePkcs8KeyDer, SubjectPublicKeyInfoDer,
    pem::{PemObject, SectionKind},
};

type PemSection = (SectionKind, Vec<u8>);

fn decode_pem(pem: &str) -> anyhow::Result<Vec<PemSection>> {
    PemSection::pem_slice_iter(pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        // PEM errors may contain input lines. Do not retain secret input in
        // either the message or the source chain.
        .map_err(|_| anyhow::anyhow!("failed to decode PEM"))
}

pub(super) fn parse_certificates(pem: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let mut certificates = Vec::new();
    for (kind, der) in decode_pem(pem)? {
        ensure!(
            kind == SectionKind::Certificate,
            "expected only certificates in TLS certificate chain"
        );
        let certificate = CertificateDer::from(der);
        rustls::server::ParsedCertificate::try_from(&certificate)
            .context("invalid X.509 certificate in TLS chain")?;
        certificates.push(certificate);
    }
    ensure!(!certificates.is_empty(), "TLS certificate chain is empty");
    Ok(certificates)
}

pub(super) fn parse_private_key(pem: &str) -> anyhow::Result<PrivatePkcs8KeyDer<'static>> {
    match decode_pem(pem)?.as_slice() {
        [(SectionKind::PrivateKey, der)] => Ok(PrivatePkcs8KeyDer::from(der.clone())),
        _ => bail!("expected exactly one unencrypted PKCS#8 private key"),
    }
}

pub(super) fn parse_public_key(pem: &str) -> anyhow::Result<SubjectPublicKeyInfoDer<'static>> {
    match decode_pem(pem)?.as_slice() {
        [(SectionKind::PublicKey, der)] => Ok(SubjectPublicKeyInfoDer::from(der.clone())),
        _ => bail!("expected exactly one SPKI public key"),
    }
}

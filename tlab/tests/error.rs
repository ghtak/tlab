use std::error::Error as _;
use tlab::{Error, Result};

#[test]
fn invalid_parameters_support_static_and_owned_messages() {
    for (message, expected) in [
        ("static".into(), "invalid parameter: static"),
        (String::from("owned").into(), "invalid parameter: owned"),
    ] {
        assert_eq!(Error::InvalidParameter(message).to_string(), expected);
    }
}

#[test]
fn internal_error_converts_and_preserves_source() {
    fn fail() -> Result<()> {
        Err(anyhow::anyhow!("root cause").context("context"))?
    }
    let error = fail().unwrap_err();
    assert_eq!(error.to_string(), "internal error: context");
    let source = error.source().unwrap();
    assert_eq!(source.to_string(), "context");
    assert_eq!(source.source().unwrap().to_string(), "root cause");
    assert!(matches!(error, Error::Internal(_)));
}

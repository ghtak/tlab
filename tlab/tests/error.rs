use std::borrow::Cow;
use std::error::Error as _;
use std::io;

use tlab::{Error, Result};

#[test]
fn illegal_state_preserves_borrowed_and_owned_messages() {
    fn owned_error() -> tlab::error::Error {
        let state = String::from("closed");
        Error::IllegalState(format!("state is {state}").into())
    }

    let borrowed = Error::IllegalState("not initialized".into());
    assert!(matches!(
        &borrowed,
        Error::IllegalState(Cow::Borrowed("not initialized"))
    ));
    assert_eq!(borrowed.to_string(), "illegal state: not initialized");
    assert!(borrowed.source().is_none());

    let owned = owned_error();
    assert!(matches!(
        &owned,
        Error::IllegalState(Cow::Owned(message)) if message == "state is closed"
    ));
    assert_eq!(owned.to_string(), "illegal state: state is closed");
}

#[test]
fn io_conversion_and_question_mark_preserve_the_original_error() {
    fn propagate(input: io::Result<()>) -> tlab::error::Result<()> {
        input?;
        Ok(())
    }

    let make_error = || io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let direct = Error::from(make_error());
    let propagated = propagate(Err(make_error())).unwrap_err();

    for error in [direct, propagated] {
        assert!(matches!(&error, Error::IoError(_)));
        let source = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
        assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(source.to_string(), "access denied");
        assert_eq!(error.to_string(), "I/O error: access denied");
    }
}

#[test]
fn internal_conversion_preserves_context_and_unclassified_cause() {
    fn propagate(input: anyhow::Result<()>) -> Result<()> {
        input?;
        Ok(())
    }

    let make_error = || anyhow::Error::new(std::fmt::Error).context("render failed");
    let direct = Error::from(make_error());
    let propagated = propagate(Err(make_error())).unwrap_err();

    for error in [direct, propagated] {
        assert_eq!(error.to_string(), "render failed");
        let source = error.source().unwrap();
        assert_eq!(source.to_string(), "render failed");
        assert!(source.source().unwrap().is::<std::fmt::Error>());

        let Error::Internal(inner) = error else {
            panic!("expected an internal error");
        };
        assert_eq!(inner.chain().count(), 2);
        assert!(inner.root_cause().is::<std::fmt::Error>());
    }
}

#[test]
fn io_wrapped_in_anyhow_remains_internal() {
    let cause = io::Error::new(io::ErrorKind::NotFound, "missing config");
    let error = Error::from(anyhow::Error::new(cause).context("load failed"));

    let Error::Internal(inner) = error else {
        panic!("an anyhow error must retain its internal classification");
    };
    let cause = inner.root_cause().downcast_ref::<io::Error>().unwrap();
    assert_eq!(cause.kind(), io::ErrorKind::NotFound);
    assert_eq!(cause.to_string(), "missing config");
}

#[test]
fn result_reexports_share_the_same_success_and_error_types() {
    fn through_module(input: Result<u32>) -> tlab::error::Result<u32> {
        let value = input?;
        Ok(value + 1)
    }

    assert_eq!(through_module(Ok(41)).unwrap(), 42);
    let error = through_module(Err(Error::IllegalState("closed".into()))).unwrap_err();
    assert!(matches!(error, Error::IllegalState(message) if message == "closed"));
}

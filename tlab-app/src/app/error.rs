//! Shared error data. Each slice owns its failure variants and their meaning.

#[derive(Debug)]
pub struct InvalidInput {
    pub field: &'static str,
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug)]
pub struct InternalFailure {
    pub stage: &'static str,
    pub cause: tlab::Error,
}

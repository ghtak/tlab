//! Reusable file settings for TLS identities and Ed25519 key pairs.
//!
//! Consumers own these settings and the returned PEM snapshots. Loading is
//! synchronous and does not install globals or watch files. Existing files are
//! never regenerated. Generation is opt-in and intended for local development.

mod ed25519;
mod files;
mod pem;
mod tls;

pub use ed25519::{Ed25519KeyFile, Ed25519Material};
pub use tls::{TlsCertFile, TlsGenerationConfig, TlsMaterial};

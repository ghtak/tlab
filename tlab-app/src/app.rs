#![allow(dead_code)]
use std::sync::Arc;

use serde::Deserialize;
use tlab::{Error, http, password};
use tokio::sync::Semaphore;

use crate::auth::passwd_policy::{NonEmptyPasswordPolicy, PasswordPolicy};

pub mod error;
pub mod response;

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    pub jwt: JwtConfig,
    pub web: crate::auth::web::Config,
    /// Shared capacity for password generation and verification.
    pub max_concurrent_password_hashes: usize,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            max_concurrent_password_hashes: 2,
            jwt: JwtConfig::default(),
            web: crate::auth::web::Config::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct JwtConfig {
    pub private_key: std::path::PathBuf,
    pub public_key: std::path::PathBuf,
    pub generate_if_missing: bool,
    pub issuer: String,
    pub audience: String,
}

impl JwtConfig {
    pub fn prepare(
        &self,
    ) -> tlab::Result<(
        Arc<crate::auth::token::AccessIssuer>,
        Arc<crate::auth::token::AccessVerifier>,
    )> {
        if self.issuer.trim().is_empty() || self.audience.trim().is_empty() {
            return Err(Error::InvalidInput(
                "auth.jwt issuer and audience are required".into(),
            ));
        }
        let material = tlab::keyfiles::Ed25519KeyFile {
            private_key: self.private_key.clone(),
            public_key: self.public_key.clone(),
            generate_if_missing: self.generate_if_missing,
        }
        .load()?;
        let issuer = crate::auth::token::AccessIssuer::new(&material, &self.issuer, &self.audience)
            .map_err(|_| Error::InvalidInput("invalid JWT configuration".into()))?;
        let verifier = crate::auth::token::AccessVerifier::new(
            material.public_key_pem(),
            &self.issuer,
            &self.audience,
        )
        .map_err(|_| Error::InvalidInput("invalid JWT configuration".into()))?;
        Ok((Arc::new(issuer), Arc::new(verifier)))
    }
}

/// The app owns the set of instances and the shared static file configuration.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub http: http::Config,
    pub https: http::Config,
    pub static_files: Option<http::StaticFilesConfig>,
}

impl ServerConfig {
    fn validate(&self) -> tlab::Result<()> {
        if self.http.tls.is_some() {
            return Err(Error::InvalidInput(
                "server.http must not contain TLS settings".into(),
            ));
        }
        if self.https.tls.is_none() {
            return Err(Error::InvalidInput(
                "server.https requires TLS settings".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub tracing: tlab::tracing::Config,
    pub server: ServerConfig,
    pub passwd_hasher: password::argon2id::Config,
    pub database: tlab::sqlxdb::Config,
    #[serde(default)]
    pub auth: AuthConfig,
}

pub mod db {
    pub type Database = tlab::sqlxdb::Database<sqlx::Postgres>;
    pub type Context<'a> = tlab::sqlxdb::Context<'a, sqlx::Postgres>;
    pub type QueryBuilder = sqlx::QueryBuilder<sqlx::Postgres>;
}

/// Owns application settings and prepares servers; the HTTP module runs them.
pub struct Container {
    pub config: Config,
    pub http: http::Server,
    pub https: http::Server,
    pub passwd_hasher: Arc<dyn password::PasswordHasher>,
    pub passwd_policy: Arc<dyn PasswordPolicy>,
    pub passwd_hash_slots: Arc<Semaphore>,
    pub local_login: crate::auth::LocalLogin,
    pub database: db::Database,
    pub access_issuer: Arc<crate::auth::token::AccessIssuer>,
    pub access_verifier: Arc<crate::auth::token::AccessVerifier>,
}

impl Container {
    pub async fn new(config: Config) -> tlab::Result<Self> {
        config.server.validate()?;
        config.auth.web.validate()?;
        if config.auth.max_concurrent_password_hashes == 0
            || config.auth.max_concurrent_password_hashes > Semaphore::MAX_PERMITS
        {
            return Err(Error::InvalidInput(
                "auth.max_concurrent_password_hashes must be between 1 and Semaphore::MAX_PERMITS"
                    .into(),
            ));
        }
        let (access_issuer, access_verifier) = config.auth.jwt.prepare()?;
        // Prepare both instances before main opens any listeners.
        let http = http::Server::new(&config.server.http).await?;
        let https = http::Server::new(&config.server.https).await?;
        let passwd_hasher = Arc::new(password::argon2id::Hasher::new(&config.passwd_hasher)?);
        let passwd_policy = Arc::new(NonEmptyPasswordPolicy);
        let passwd_hash_slots =
            Arc::new(Semaphore::new(config.auth.max_concurrent_password_hashes));
        let local_login = crate::auth::LocalLogin::new(
            passwd_hasher.clone(),
            passwd_hash_slots.clone(),
            access_issuer.clone(),
        )
        .await?;
        let database = db::Database::new(&config.database).await?;
        Ok(Self {
            config,
            http,
            https,
            passwd_hasher,
            passwd_policy,
            passwd_hash_slots,
            local_login,
            database,
            access_issuer,
            access_verifier,
        })
    }
}

impl axum::extract::FromRef<Arc<Container>> for crate::auth::token::AccessVerifier {
    fn from_ref(container: &Arc<Container>) -> Self {
        (*container.access_verifier).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_debug_hides_nested_database_url() {
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "tracing": {},
            "server": {
                "http": {"bind": "127.0.0.1:0"},
                "https": {"bind": "127.0.0.1:0", "tls": {
                    "certificate_chain": "chain.pem", "private_key": "key.pem"
                }}
            },
            "passwd_hasher": {},
            "database": {"url": "", "max_connections": 7}
        }))
        .unwrap();
        config.database.url = "postgres://nested-user:nested-secret@private-host/private-db".into();
        for output in [format!("{config:?}"), format!("{config:#?}")] {
            for private in [
                "postgres://",
                "nested-user",
                "nested-secret",
                "private-host",
                "private-db",
            ] {
                assert!(
                    !output.contains(private),
                    "nested database URL must be hidden"
                );
            }
        }
    }

    #[tokio::test]
    async fn container_rejects_protocol_mismatch_before_preparing_tls() {
        for server in [
            serde_json::json!({
                "http": {"bind": "127.0.0.1:0", "tls": {
                    "certificate_chain": "chain.pem", "private_key": "key.pem"
                }},
                "https": {"bind": "127.0.0.1:0", "tls": {
                    "certificate_chain": "chain.pem", "private_key": "key.pem"
                }}
            }),
            serde_json::json!({
                "http": {"bind": "127.0.0.1:0"},
                "https": {"bind": "127.0.0.1:0"}
            }),
        ] {
            let config: Config = serde_json::from_value(serde_json::json!({
                "tracing": {}, "server": server, "passwd_hasher": {},
                "database": {"url": "postgres://localhost/test", "max_connections": 5}
            }))
            .unwrap();
            assert!(matches!(
                Container::new(config).await,
                Err(Error::InvalidInput(_))
            ));
        }
    }

    #[tokio::test]
    async fn jwt_configuration_fails_before_connecting_to_database() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
        let config: Config = tlab::config::load(path, "TLAB_JWT_SETTINGS_TEST").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut invalid = config.clone();
        invalid.auth.jwt.private_key = dir.path().join("missing-private");
        invalid.auth.jwt.public_key = dir.path().join("missing-public");
        assert!(Container::new(invalid.clone()).await.is_err());
        invalid.auth.jwt.generate_if_missing = true;
        invalid.auth.jwt.issuer.clear();
        assert!(Container::new(invalid.clone()).await.is_err());
        assert!(!invalid.auth.jwt.private_key.exists());
        invalid.auth.jwt.issuer = "issuer".into();
        invalid.auth.jwt.audience.clear();
        assert!(Container::new(invalid).await.is_err());
    }

    #[test]
    fn server_settings_require_both_instances() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"http": {"bind": "127.0.0.1:0"}}),
            serde_json::json!({"https": {"bind": "127.0.0.1:0", "tls": {
                "certificate_chain": "chain.pem", "private_key": "key.pem"
            }}}),
        ] {
            assert!(serde_json::from_value::<ServerConfig>(value).is_err());
        }
    }

    #[test]
    fn checked_in_settings_enable_local_http_and_https() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
        let config: Config = tlab::config::load(path, "TLAB_APP_SETTINGS_TEST").unwrap();
        config.server.validate().unwrap();
        assert_eq!(config.server.http.bind, "127.0.0.1:8080".parse().unwrap());
        let https = config.server.https;
        assert_eq!(https.bind, "127.0.0.1:8081".parse().unwrap());
        assert!(https.tls.is_some());
        assert!(config.server.static_files.is_none());
        password::argon2id::Hasher::new(&config.passwd_hasher).unwrap();
        assert_eq!(config.auth.max_concurrent_password_hashes, 2);
    }

    #[tokio::test]
    async fn invalid_hash_concurrency_is_rejected_before_resources_are_prepared() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/app.toml");
        let config: Config = tlab::config::load(path, "TLAB_APP_AUTH_SETTINGS_TEST").unwrap();
        for count in [0, Semaphore::MAX_PERMITS + 1] {
            let mut invalid = config.clone();
            invalid.auth.max_concurrent_password_hashes = count;
            assert!(matches!(
                Container::new(invalid).await,
                Err(Error::InvalidInput(_))
            ));
        }
    }
}

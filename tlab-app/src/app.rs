#![allow(dead_code)]
use std::sync::Arc;

use serde::Deserialize;
use tlab::{Error, http, password};
use tokio::sync::Semaphore;

use crate::auth::passwd_policy::{NonEmptyPasswordPolicy, PasswordPolicy};

pub mod error;
pub mod response;

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
    pub auth: crate::auth::Config,
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
    pub database: Arc<db::Database>,
    pub(crate) role_permission_resolver: Arc<dyn crate::rbac::RolePermissionResolver>,
    pub access_issuer: Arc<crate::auth::jwt::AccessIssuer>,
    pub access_verifier: Arc<crate::auth::jwt::AccessVerifier>,
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
        let database = Arc::new(db::Database::new(&config.database).await?);
        let role_permission_resolver =
            Arc::new(crate::rbac::DbRolePermissionResolver::new(database.clone()));
        Ok(Self {
            config,
            http,
            https,
            passwd_hasher,
            passwd_policy,
            passwd_hash_slots,
            local_login,
            database,
            role_permission_resolver,
            access_issuer,
            access_verifier,
        })
    }
}

impl axum::extract::FromRef<Arc<Container>> for crate::auth::jwt::AccessVerifier {
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

    fn config_for_jwt_startup_test(jwt: crate::auth::jwt::Config) -> Config {
        let mut config: Config = serde_json::from_value(serde_json::json!({
            "tracing": {},
            "server": {
                "http": {"bind": "127.0.0.1:0"},
                "https": {"bind": "127.0.0.1:0", "tls": {
                    "certificate_chain": "unused-chain.pem", "private_key": "unused-key.pem"
                }}
            },
            "passwd_hasher": {},
            "database": {"url": "", "max_connections": 5}
        }))
        .unwrap();
        config.auth.jwt = jwt;
        config
    }

    #[tokio::test]
    async fn missing_jwt_keys_with_generation_disabled_fail_before_database() {
        let dir = tempfile::tempdir().unwrap();
        let jwt = crate::auth::jwt::Config {
            private_key: dir.path().join("private.pem"),
            public_key: dir.path().join("public.pem"),
            generate_if_missing: false,
            issuer: "issuer".into(),
            audience: "audience".into(),
        };
        let result = Container::new(config_for_jwt_startup_test(jwt.clone())).await;
        assert!(
            matches!(result, Err(Error::Internal(ref error)) if error.to_string().contains("failed to prepare Ed25519 files"))
        );
        assert!(!jwt.private_key.exists());
        assert!(!jwt.public_key.exists());
    }

    #[tokio::test]
    async fn empty_jwt_issuer_or_audience_fails_before_key_generation_and_database() {
        for (issuer, audience) in [("", "audience"), ("issuer", "")] {
            let dir = tempfile::tempdir().unwrap();
            let jwt = crate::auth::jwt::Config {
                private_key: dir.path().join("private.pem"),
                public_key: dir.path().join("public.pem"),
                generate_if_missing: true,
                issuer: issuer.into(),
                audience: audience.into(),
            };
            let result = Container::new(config_for_jwt_startup_test(jwt.clone())).await;
            assert!(
                matches!(result, Err(Error::InvalidInput(ref message)) if message == "auth.jwt issuer and audience are required")
            );
            assert!(!jwt.private_key.exists());
            assert!(!jwt.public_key.exists());
        }
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

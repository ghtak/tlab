use serde::Deserialize;
use tlab::{Error, http, password};

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
}

/// Owns application settings and prepares servers; main owns execution.
pub struct Container {
    pub config: Config,
    pub http: http::Server,
    pub https: http::Server,
    pub passwd_hasher: Box<dyn password::PasswordHasher>,
}

impl Container {
    pub async fn new(config: Config) -> tlab::Result<Self> {
        config.server.validate()?;
        // Prepare both instances before main opens any listeners.
        let http = http::Server::new(&config.server.http).await?;
        let https = http::Server::new(&config.server.https).await?;
        let passwd_hasher = Box::new(password::argon2id::Hasher::new(&config.passwd_hasher)?);
        Ok(Self {
            config,
            http,
            https,
            passwd_hasher,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                "tracing": {}, "server": server, "passwd_hasher": {}
            }))
            .unwrap();
            assert!(matches!(
                Container::new(config).await,
                Err(Error::InvalidInput(_))
            ));
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
    }
}

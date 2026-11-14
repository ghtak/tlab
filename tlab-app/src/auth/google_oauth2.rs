use serde::Deserialize;

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
    #[serde(deserialize_with = "deserialize_redirect_uris")]
    pub redirect_uris: Vec<String>,
}

fn deserialize_redirect_uris<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RedirectUris {
        List(Vec<String>),
        CommaSeparated(String),
    }

    Ok(match RedirectUris::deserialize(deserializer)? {
        RedirectUris::List(uris) => uris,
        RedirectUris::CommaSeparated(uris) => uris
            .split(',')
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .field("auth_uri", &self.auth_uri)
            .field("token_uri", &self.token_uri)
            .field("redirect_uris", &self.redirect_uris)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris_accept_array_and_environment_string() {
        for value in [
            serde_json::json!(["https://one", "https://two"]),
            serde_json::json!("https://one, https://two"),
        ] {
            let config: Config = serde_json::from_value(serde_json::json!({
                "client_id": "id", "client_secret": "private-value-123", "auth_uri": "auth",
                "token_uri": "token", "redirect_uris": value
            }))
            .unwrap();
            assert_eq!(config.redirect_uris, ["https://one", "https://two"]);
            assert!(!format!("{config:?}").contains("private-value-123"));
        }
    }
}

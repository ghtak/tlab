pub mod entity;
mod extractor;
mod google_oauth2;
pub mod jwt;
mod local_account;
pub mod passwd_policy;
mod repository;
mod route;
mod session_policy;
mod slice;
#[cfg(test)]
pub(crate) mod test_support;
pub mod token;
pub(crate) mod web;

pub(crate) use local_account::{
    LocalAccountInput, hash_local_account, insert_prepared_local_account, prepare_local_account,
};
pub(crate) use repository::account::find_by_id as find_account;
pub(crate) use repository::account::lock_by_id as lock_account;
pub(crate) use route::{api_routes, web_protected_routes, web_public_routes};
pub(crate) use session_policy::allowed as account_allowed;
#[allow(unused_imports, reason = "Token login is connected by API step 6")]
pub use slice::login_local::{Command as LoginCommand, LocalLogin, LoginTokens};

#[derive(Debug, serde::Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub jwt: jwt::Config,
    pub google_oauth2: google_oauth2::Config,
    pub web: web::Config,
    /// Shared capacity for password generation and verification.
    pub max_concurrent_password_hashes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_concurrent_password_hashes: 2,
            jwt: jwt::Config::default(),
            google_oauth2: google_oauth2::Config::default(),
            web: web::Config::default(),
        }
    }
}

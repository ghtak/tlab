pub mod entity;
mod extractor;
pub mod passwd_policy;
mod repository;
mod route;
mod session_policy;
mod slice;
#[cfg(test)]
mod test_support;
pub mod token;
pub(crate) mod web;

pub use route::router;
#[allow(unused_imports, reason = "Token login is connected by API step 6")]
pub use slice::login_local::{
    Command as LoginCommand, Error as LoginError, LocalLogin, LoginTokens,
};

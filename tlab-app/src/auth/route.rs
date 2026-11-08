use std::sync::Arc;

use axum::{Router, routing::post};

use crate::app::Container;

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .route(
            "/api/v1/accounts/local",
            post(super::slice::create_local_user::create_local_user),
        )
        .route("/api/v1/auth/local", post(super::slice::login_local::login))
        .route(
            "/api/v1/auth/refresh",
            post(super::slice::refresh_access_token::refresh),
        )
        .route("/api/v1/auth/logout", post(super::slice::logout::logout))
}

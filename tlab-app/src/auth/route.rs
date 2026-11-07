use std::sync::Arc;

use axum::{Router, routing::post};

use crate::app::Container;

pub fn router() -> Router<Arc<Container>> {
    Router::new()
        .route(
            "/api/v1/accounts/local",
            post(super::create_local_user::create_local_user),
        )
        .route("/api/v1/auth/local", post(super::login_local::login))
}

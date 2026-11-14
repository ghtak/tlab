use std::sync::Arc;

use axum::{
    Router,
    http::{StatusCode, header::WWW_AUTHENTICATE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;

use crate::{
    app::{Container, response},
    auth::jwt::AccessClaims,
};

pub(crate) fn api_routes() -> Router<Arc<Container>> {
    Router::new()
        .route(
            "/accounts/local",
            post(super::slice::create_local_user::create_local_user),
        )
        .route("/auth/local", post(super::slice::login_local::login))
        .route(
            "/auth/refresh",
            post(super::slice::refresh_access_token::refresh),
        )
        .route("/auth/logout", post(super::slice::logout::logout))
        .route("/auth/me", get(me))
}

pub(crate) fn web_public_routes() -> Router<Arc<Container>> {
    Router::new()
        .route("/auth/local", post(super::slice::login_local::web_login))
        .route("/auth/logout", post(super::slice::logout::web_logout))
}

pub(crate) fn web_protected_routes() -> Router<Arc<Container>> {
    Router::new().route("/auth/me", get(web_me)).route(
        "/accounts/local",
        post(super::slice::create_local_user::create_local_user),
    )
}

async fn web_me(claims: AccessClaims) -> Response {
    response::success(
        StatusCode::OK,
        json!({"sub": claims.sub, "sid": claims.sid, "user": claims.user}),
    )
}

async fn me(claim: Option<AccessClaims>) -> Response {
    match claim {
        Some(claim) => (
            StatusCode::OK,
            axum::Json(json!({"sid": claim.sid, "sub": claim.sub})),
        )
            .into_response(),
        None => (
            [(WWW_AUTHENTICATE, "Bearer")],
            response::error(
                StatusCode::UNAUTHORIZED,
                "INVALID_ACCESS_TOKEN",
                "authentication required",
            ),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{
            Request,
            header::{AUTHORIZATION, WWW_AUTHENTICATE},
        },
    };
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn me_maps_optional_authentication_to_identity_or_common_error() {
        let (issuer, verifier) = crate::auth::test_support::token_keys();
        let sid = uuid::Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let valid = issuer.issue("123", sid, Default::default(), now).unwrap();
        let expired = issuer
            .issue("123", sid, Default::default(), now - 600)
            .unwrap();
        let router = Router::new()
            .route("/auth/me", get(me))
            .with_state(verifier);
        for (token, expected_status) in [
            (Some(valid.expose_secret()), StatusCode::OK),
            (None, StatusCode::UNAUTHORIZED),
            (Some("invalid"), StatusCode::UNAUTHORIZED),
            (Some(expired.expose_secret()), StatusCode::UNAUTHORIZED),
        ] {
            let mut request = Request::get("/auth/me");
            if let Some(token) = token {
                request = request.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected_status);
            if expected_status == StatusCode::UNAUTHORIZED {
                assert_eq!(response.headers()[WWW_AUTHENTICATE], "Bearer");
            }
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            if expected_status == StatusCode::OK {
                assert_eq!(body, json!({"sub": "123", "sid": sid}));
            } else {
                assert_eq!(
                    body,
                    json!({"code": "INVALID_ACCESS_TOKEN", "message": "authentication required"})
                );
            }
        }
    }
}

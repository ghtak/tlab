use axum::response::IntoResponse;
use std::sync::Arc;

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::Response,
};

use crate::app::Container;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::auth) struct RefreshRequest {
    refresh_token: String,
}

pub(in crate::auth) async fn refresh(
    State(container): State<Arc<Container>>,
    payload: Result<Json<RefreshRequest>, JsonRejection>,
) -> Response {
    let response = match payload {
        Err(e) => crate::app::response::invalid_json(e),
        Ok(Json(input)) => match super::usecase::execute(
            &container.database,
            &input.refresh_token,
            &container.access_issuer,
        )
        .await
        {
            Ok(token) => crate::app::response::success(
                StatusCode::OK,
                serde_json::json!({"access_token": token.expose_secret()}),
            ),
            Err(e) => e.into_response(),
        },
    };
    no_store(response)
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

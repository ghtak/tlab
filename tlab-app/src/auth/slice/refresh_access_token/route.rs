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
            Err(e) => operation_error(e),
        },
    };
    no_store(response)
}

fn operation_error(error: super::usecase::Error) -> Response {
    match error {
        super::usecase::Error::InvalidRefresh => crate::app::response::error(
            StatusCode::UNAUTHORIZED,
            "INVALID_REFRESH_TOKEN",
            None,
            "refresh token rejected",
        ),
        super::usecase::Error::Internal(failure) => {
            tracing::error!(operation = "refresh_access_token", stage = failure.stage, cause = ?failure.cause, "refresh operation failed");
            crate::app::response::internal_error("refresh operation failed")
        }
    }
}
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

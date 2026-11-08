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
pub(in crate::auth) struct LogoutRequest {
    refresh_token: String,
}

pub(in crate::auth) async fn logout(
    State(container): State<Arc<Container>>,
    payload: Result<Json<LogoutRequest>, JsonRejection>,
) -> Response {
    let response = match payload {
        Err(e) => crate::app::response::invalid_json(e),
        Ok(Json(input)) => {
            match super::usecase::execute(&container.database, &input.refresh_token).await {
                Ok(()) => crate::app::response::success(StatusCode::OK, serde_json::json!({})),
                Err(e) => operation_error(e),
            }
        }
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
            tracing::error!(operation = "logout", stage = failure.stage, cause = ?failure.cause, "refresh operation failed");
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

pub(in crate::auth) async fn web_logout(
    State(container): State<Arc<Container>>,
    cookies: tower_cookies::Cookies,
) -> Response {
    use crate::auth::web;
    let result = match cookies.get(container.config.auth.web.refresh_name()) {
        None => crate::app::response::success(StatusCode::OK, serde_json::json!({})),
        Some(refresh) => {
            match super::usecase::execute(&container.database, refresh.value()).await {
                Ok(()) | Err(super::usecase::Error::InvalidRefresh) => {
                    crate::app::response::success(StatusCode::OK, serde_json::json!({}))
                }
                Err(error) => return web::no_store(operation_error(error)),
            }
        }
    };
    web::clear(&cookies, &container.config.auth.web);
    web::no_store(result)
}

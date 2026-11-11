use std::sync::Arc;

use axum::Router;

use crate::{app::Container, auth};

pub(super) fn router() -> Router<Arc<Container>> {
    Router::new()
        .merge(auth::api_routes())
        .merge(super::shared_routes())
}

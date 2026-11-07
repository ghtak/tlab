use std::sync::Arc;

use axum::Router;

use crate::app::Container;

pub fn router() -> Router<Arc<Container>> {
    Router::new().merge(super::create_local_user::router())
}

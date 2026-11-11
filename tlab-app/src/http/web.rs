use std::sync::Arc;

use axum::Router;

use crate::{app::Container, auth};

pub(super) fn router(container: Arc<Container>) -> Router<Arc<Container>> {
    let protected = Router::new()
        .merge(auth::web_protected_routes())
        .merge(super::shared_routes());
    let protected = auth::web::with_authentication(protected, container.clone());
    let web = Router::new()
        .merge(auth::web_public_routes())
        .merge(protected);
    auth::web::with_web_transport(web, container)
}

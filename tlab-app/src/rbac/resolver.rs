use std::sync::Arc;

use async_trait::async_trait;

use crate::app::db::Database;

use super::{PermissionSet, RoleIds, resolve_in_context};

/// Replaceable role-to-permission lookup; callers retain the roles from Access.
#[async_trait]
pub(crate) trait RolePermissionResolver: Send + Sync {
    async fn resolve(&self, role_ids: &RoleIds) -> tlab::Result<PermissionSet>;
}

pub(crate) struct DbRolePermissionResolver {
    database: Arc<Database>,
}

impl DbRolePermissionResolver {
    /// Share the application's existing pool without opening a second pool.
    pub(crate) fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

#[async_trait]
impl RolePermissionResolver for DbRolePermissionResolver {
    async fn resolve(&self, role_ids: &RoleIds) -> tlab::Result<PermissionSet> {
        if role_ids.as_slice().is_empty() {
            return Ok(PermissionSet::default());
        }
        let mut conn = self.database.conn().await?;
        resolve_in_context(&mut conn.context(), role_ids).await
    }
}

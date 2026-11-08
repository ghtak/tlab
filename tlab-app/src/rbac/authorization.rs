use super::{Actor, PermissionCode, RolePermissionResolver, require_in_set};

/// False is a policy denial; Err preserves lookup failure for the caller.
pub(crate) async fn require_permission(
    actor: &Actor,
    required: &PermissionCode,
    resolver: &dyn RolePermissionResolver,
) -> tlab::Result<bool> {
    let permissions = resolver.resolve(&actor.role_ids).await?;
    Ok(require_in_set(&permissions, required))
}

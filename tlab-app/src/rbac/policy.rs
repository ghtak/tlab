use super::{PermissionCode, PermissionSet};

pub(crate) fn require_in_set(actor_permissions: &PermissionSet, required: &PermissionCode) -> bool {
    actor_permissions.contains(required)
}

/// Only newly added permissions are delegated. Stronger existing permissions
/// may remain or be removed, provided the actor can manage roles.
pub(crate) fn check_role_edit(
    actor_permissions: &PermissionSet,
    before: &PermissionSet,
    after: &PermissionSet,
) -> bool {
    require_in_set(
        actor_permissions,
        &PermissionCode("rbac.role.manage".into()),
    ) && after.difference(before).is_subset(actor_permissions)
}

pub(crate) fn check_assignment_grant(
    actor_permissions: &PermissionSet,
    target_permissions: &PermissionSet,
) -> bool {
    require_in_set(
        actor_permissions,
        &PermissionCode("rbac.assignment.manage".into()),
    ) && target_permissions.is_subset(actor_permissions)
}

//! Application RBAC contracts. Runtime assembly belongs to step 07.
mod authorization;
mod claims;
mod entity;
mod policy;
pub(crate) mod repository;
mod resolver;

pub(crate) use authorization::require_permission;
pub(crate) use claims::Actor;
pub(crate) use entity::*;
pub(crate) use policy::{check_assignment_grant, check_role_edit, require_in_set};
pub(crate) use repository::{account_role::list_role_ids, role_permission::resolve_in_context};
pub(crate) use resolver::{DbRolePermissionResolver, RolePermissionResolver};

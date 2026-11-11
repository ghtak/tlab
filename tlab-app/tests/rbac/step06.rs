// Test-only module assembly until step 07 registers the new sources.
//! Application RBAC contracts. Runtime assembly belongs to step 07.
#[path = "../../src/rbac/authorization.rs"]
mod authorization;
#[path = "../../src/rbac/claims.rs"]
mod claims;
#[path = "../../src/rbac/entity.rs"]
mod entity;
#[path = "../../src/rbac/policy.rs"]
mod policy;
#[path = "../../src/rbac/repository/mod.rs"]
pub(crate) mod repository;
#[path = "../../src/rbac/resolver.rs"]
mod resolver;
#[path = "slices06.rs"]
pub(crate) mod slice;

pub(crate) use authorization::require_permission;
pub(crate) use claims::Actor;
pub(crate) use entity::*;
pub(crate) use policy::{check_assignment_grant, check_role_edit, require_in_set};
pub(crate) use repository::{account_role::list_role_ids, role_permission::resolve_in_context};
pub(crate) use resolver::{DbRolePermissionResolver, RolePermissionResolver};

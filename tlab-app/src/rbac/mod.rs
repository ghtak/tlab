//! Application RBAC storage contracts. Runtime assembly belongs to step 07.
//! Future resolver/claims/policy/slice/route declarations are added when their
//! real files are handed over; absent modules are deliberately not stubbed.
mod entity;
pub(crate) mod repository;

pub(crate) use entity::*;
pub(crate) use repository::{account_role::list_role_ids, role_permission::resolve_in_context};

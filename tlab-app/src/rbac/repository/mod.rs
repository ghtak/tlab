//! Storage only. Callers own transactions, account/role lock order, permission
//! decisions, version conflicts, replacement atomicity and assignment limits.
pub(crate) mod account_role;
pub(crate) mod permission;
pub(crate) mod role;
pub(crate) mod role_permission;

use super::entity::{Account, AccountStatus};

pub fn allowed(account: &Account) -> bool {
    matches!(
        account.status,
        AccountStatus::Registered | AccountStatus::Active
    )
}

pub(crate) fn user_claim(
    account: &Account,
    role_ids: &crate::rbac::RoleIds,
) -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({"nickname": account.nickname, "status": account.status, "role_ids": role_ids})
        .as_object()
        .expect("object literal")
        .clone()
}

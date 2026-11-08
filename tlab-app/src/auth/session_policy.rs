use super::entity::{Account, AccountStatus};

pub fn allowed(account: &Account) -> bool {
    matches!(
        account.status,
        AccountStatus::Registered | AccountStatus::Active
    )
}

pub fn user_claim(account: &Account) -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({"nickname": account.nickname, "status": account.status})
        .as_object()
        .expect("object literal")
        .clone()
}

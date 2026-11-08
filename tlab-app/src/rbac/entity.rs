use sqlx::types::time::OffsetDateTime;
use std::collections::BTreeSet;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, sqlx::Type,
)]
#[serde(transparent)]
#[sqlx(transparent)]
pub(crate) struct RoleId(pub i64);
impl RoleId {
    pub(crate) fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct RoleIds(Vec<RoleId>);
impl RoleIds {
    pub(crate) fn from_raw(mut values: Vec<i64>) -> Self {
        values.sort_unstable();
        values.dedup();
        Self(values.into_iter().map(RoleId).collect())
    }
    pub(crate) fn as_slice(&self) -> &[RoleId] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub(crate) struct PermissionCode(pub String);
impl PermissionCode {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct PermissionSet(BTreeSet<PermissionCode>);
impl PermissionSet {
    pub(crate) fn from_codes(values: impl IntoIterator<Item = PermissionCode>) -> Self {
        Self(values.into_iter().collect())
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &PermissionCode> {
        self.0.iter()
    }
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn contains(&self, code: &PermissionCode) -> bool {
        self.0.contains(code)
    }
    pub(crate) fn is_subset(&self, other: &Self) -> bool {
        self.0.is_subset(&other.0)
    }
    pub(crate) fn union(&self, other: &Self) -> Self {
        Self::from_codes(self.0.union(&other.0).cloned())
    }
    pub(crate) fn difference(&self, other: &Self) -> Self {
        Self(self.0.difference(&other.0).cloned().collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct Role {
    pub id: RoleId,
    pub name: String,
    pub description: String,
    pub version: i64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
pub(crate) struct NewRole {
    pub name: String,
    pub description: String,
}
pub(crate) struct RoleMetadata {
    pub name: String,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct Permission {
    pub code: PermissionCode,
    pub description: String,
}
pub(crate) struct NewRolePermission {
    pub role_id: RoleId,
    pub permission_code: PermissionCode,
}
pub(crate) struct NewAccountRole {
    pub account_id: i64,
    pub role_id: RoleId,
}
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct AccountRole {
    pub account_id: i64,
    pub role_id: RoleId,
    pub granted_at: OffsetDateTime,
}

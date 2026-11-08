use crate::auth::token::AccessClaims;

use super::RoleIds;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Actor {
    pub account_id: i64,
    pub role_ids: RoleIds,
}

impl Actor {
    /// Call only after AccessVerifier has verified the JWT. This interprets the
    /// RBAC boundary; it does not verify signatures, time or account state.
    pub(crate) fn try_from_verified_claims(claims: &AccessClaims) -> tlab::Result<Self> {
        let invalid = || tlab::Error::InvalidInput("invalid RBAC claims".into());
        let account_id = claims.sub.parse::<i64>().map_err(|_| invalid())?;
        let role_ids = match claims.user.get("role_ids") {
            None => RoleIds::default(),
            Some(value) => {
                let values = value.as_array().ok_or_else(invalid)?;
                let ids = values
                    .iter()
                    .map(|value| value.as_i64().filter(|id| *id > 0).ok_or_else(invalid))
                    .collect::<tlab::Result<Vec<_>>>()?;
                RoleIds::from_raw(ids)
            }
        };
        Ok(Self {
            account_id,
            role_ids,
        })
    }
}

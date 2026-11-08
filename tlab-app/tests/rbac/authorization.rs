use super::{code, isolated, new_role, rbac::*, token};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

#[test]
fn verified_claims_apply_only_rbac_identity_and_json_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let material = tlab::keyfiles::Ed25519KeyFile {
        private_key: dir.path().join("private.pem"),
        public_key: dir.path().join("public.pem"),
        generate_if_missing: true,
    }
    .load()
    .unwrap();
    let issuer = token::AccessIssuer::new(&material, "issuer", "audience").unwrap();
    let verifier =
        token::AccessVerifier::new(material.public_key_pem(), "issuer", "audience").unwrap();
    let interpret = |sub: &str, user: Value| {
        let signed = issuer
            .issue(
                sub,
                uuid::Uuid::new_v4(),
                user.as_object().unwrap().clone(),
                100,
            )
            .unwrap();
        let verified = verifier.verify(signed.expose_secret(), 100).unwrap();
        Actor::try_from_verified_claims(&verified)
    };

    let actor = interpret("9223372036854775807", json!({"role_ids": [3, 1, 3]})).unwrap();
    assert_eq!(actor.account_id, i64::MAX);
    assert_eq!(actor.role_ids, RoleIds::from_raw(vec![1, 3]));
    for user in [json!({}), json!({"role_ids": []})] {
        assert_eq!(interpret("1", user).unwrap().role_ids, RoleIds::default());
    }
    for (name, value) in [
        ("null", json!(null)),
        ("object", json!({})),
        ("scalar", json!(1)),
        ("string", json!(["1"])),
        ("float", json!([1.0])),
        ("negative", json!([-1])),
        ("zero", json!([0])),
        ("overflow", json!([9223372036854775808u64])),
        ("mixed", json!([1, true])),
    ] {
        assert!(
            matches!(
                interpret("1", json!({"role_ids": value})),
                Err(tlab::Error::InvalidInput(_))
            ),
            "{name}"
        );
    }
    assert!(matches!(
        interpret("not-an-account-id", json!({})),
        Err(tlab::Error::InvalidInput(_))
    ));
}

fn permissions(codes: &[&str]) -> PermissionSet {
    PermissionSet::from_codes(codes.iter().map(|value| code(value)))
}

#[test]
fn management_requires_action_permission_and_limits_only_new_delegation() {
    let actor = permissions(&["rbac.role.manage", "rbac.assignment.manage", "rbac.read"]);
    let stronger = permissions(&["account.create", "rbac.read"]);
    assert!(check_role_edit(
        &actor,
        &stronger,
        &permissions(&["account.create"])
    ));
    assert!(check_role_edit(&actor, &stronger, &stronger));
    assert!(check_role_edit(
        &actor,
        &PermissionSet::default(),
        &permissions(&["rbac.read"])
    ));
    assert!(!check_role_edit(
        &actor,
        &PermissionSet::default(),
        &stronger
    ));
    assert!(check_assignment_grant(&actor, &permissions(&["rbac.read"])));
    assert!(!check_assignment_grant(&actor, &stronger));

    // Revocation, metadata changes and deletion require only the action
    // permission; they do not compare the stronger target with actor holdings.
    assert!(require_in_set(&actor, &code("rbac.assignment.manage")));
    assert!(require_in_set(&actor, &code("rbac.role.manage")));
    let no_management = permissions(&["rbac.read", "account.create"]);
    assert!(!check_role_edit(
        &no_management,
        &stronger,
        &PermissionSet::default()
    ));
    assert!(!check_assignment_grant(
        &no_management,
        &permissions(&["rbac.read"])
    ));
    assert!(!require_in_set(
        &no_management,
        &code("rbac.assignment.manage")
    ));
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn dyn_db_resolver_authorizes_current_union_and_preserves_lookup_failures() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        let a = repository::role::insert(&mut conn.context(), &new_role("a"))
            .await
            .unwrap();
        let b = repository::role::insert(&mut conn.context(), &new_role("b"))
            .await
            .unwrap();
        for (role_id, codes) in [
            (a.id, vec!["rbac.read"]),
            (b.id, vec!["rbac.read", "account.create"]),
        ] {
            for permission in codes {
                repository::role_permission::insert(
                    &mut conn.context(),
                    &NewRolePermission {
                        role_id,
                        permission_code: code(permission),
                    },
                )
                .await
                .unwrap();
            }
        }
        let actor = Actor {
            account_id: 1,
            role_ids: RoleIds::from_raw(vec![a.id.get(), b.id.get(), a.id.get(), i64::MAX]),
        };
        let resolver: Arc<dyn RolePermissionResolver> =
            Arc::new(DbRolePermissionResolver::new(Arc::new(db)));
        assert_eq!(
            resolver.resolve(&actor.role_ids).await.unwrap(),
            permissions(&["account.create", "rbac.read"])
        );
        assert!(
            require_permission(&actor, &code("account.create"), resolver.as_ref())
                .await
                .unwrap()
        );
        assert!(
            !require_permission(&actor, &code("rbac.role.manage"), resolver.as_ref())
                .await
                .unwrap()
        );
        // A deleted role contributes nothing, while another source of the same
        // permission survives. Permission edits affect this same Access actor.
        repository::role::delete(&mut conn.context(), a.id, 1)
            .await
            .unwrap();
        assert!(
            require_permission(&actor, &code("rbac.read"), resolver.as_ref())
                .await
                .unwrap()
        );
        repository::role_permission::delete(&mut conn.context(), b.id, &code("account.create"))
            .await
            .unwrap();
        assert!(
            !require_permission(&actor, &code("account.create"), resolver.as_ref())
                .await
                .unwrap()
        );
        let empty = Actor {
            account_id: 1,
            role_ids: RoleIds::default(),
        };
        assert!(
            !require_permission(&empty, &code("rbac.read"), resolver.as_ref())
                .await
                .unwrap()
        );

        sqlx::query("ALTER TABLE tlab_rbac_role_permission RENAME TO unavailable_permissions")
            .execute(conn.context().backend())
            .await
            .unwrap();
        let error = require_permission(&actor, &code("rbac.read"), resolver.as_ref())
            .await
            .unwrap_err();
        let tlab::Error::DbError(sqlx::Error::Database(error)) = error else {
            panic!("expected original SQL error")
        };
        assert_eq!(error.code().as_deref(), Some("42P01"));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL; checks SQLx pool timeout"]
async fn empty_roles_skip_acquisition_and_acquisition_errors_are_preserved() {
    isolated(|db| async move {
        // Exhaust this isolated pool to observe a real acquisition failure.
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(db.conn().await.unwrap());
        }
        let resolver: Arc<dyn RolePermissionResolver> =
            Arc::new(DbRolePermissionResolver::new(Arc::new(db)));
        let empty = tokio::time::timeout(
            Duration::from_secs(1),
            resolver.resolve(&RoleIds::default()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(empty, PermissionSet::default());
        assert!(matches!(
            resolver.resolve(&RoleIds::from_raw(vec![1])).await,
            Err(tlab::Error::DbError(sqlx::Error::PoolTimedOut))
        ));
        drop(held);
    })
    .await;
}

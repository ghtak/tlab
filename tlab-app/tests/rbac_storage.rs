//! Step-02/03 executable boundary: compile the actual RBAC and token sources without
//! editing step-07's binary root. These aliases exactly match app::db.
#[path = "../src/app/error.rs"]
#[allow(dead_code)]
mod app_error;
#[path = "../src/app/response.rs"]
#[allow(dead_code)]
mod app_response;
#[path = "../src/auth/repository/account.rs"]
#[allow(dead_code)]
mod auth_account;
#[path = "../src/auth/entity.rs"]
#[allow(dead_code)]
mod auth_entity;
#[path = "../src/auth/session_policy.rs"]
#[allow(dead_code)]
mod auth_session_policy;
use auth_entity as entity;
#[path = "../src/auth/token.rs"]
mod token;
mod auth {
    pub(crate) use crate::auth_account::lock_by_id as lock_account;
    pub(crate) use crate::auth_entity as entity;
    pub(crate) use crate::auth_session_policy::allowed as account_allowed;
    pub(crate) use crate::token;
}
#[path = "rbac/authorization.rs"]
mod authorization;
mod app {
    pub(crate) use crate::app_error as error;
    pub(crate) use crate::app_response as response;
    pub struct Container {
        pub database: db::Database,
        pub role_permission_resolver: std::sync::Arc<dyn crate::rbac::RolePermissionResolver>,
    }
    pub mod db {
        pub type Database = tlab::sqlxdb::Database<sqlx::Postgres>;
        pub type Context<'a> = tlab::sqlxdb::Context<'a, sqlx::Postgres>;
    }
}
#[allow(
    dead_code,
    unused_imports,
    reason = "Public contracts for later RBAC consumers are not all used by this storage harness"
)]
#[path = "../src/rbac/mod.rs"]
mod rbac;

use app::db::Database;
use rbac::{
    repository::{account_role, permission, role, role_permission},
    *,
};
use std::{future::Future, path::Path};

async fn isolated<F, Fut>(scenario: F)
where
    F: FnOnce(Database) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let url = std::env::var("TLAB_TEST_DATABASE_URL").expect("set TLAB_TEST_DATABASE_URL");
    let admin = Database::new(&tlab::sqlxdb::Config {
        url: url.clone(),
        max_connections: 1,
    })
    .await
    .unwrap();
    let schema = format!("tlab_rbac_test_{}", uuid::Uuid::new_v4().simple());
    let mut conn = admin.conn().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    let separator = if url.contains('?') { '&' } else { '?' };
    let scoped = format!("{url}{separator}options=-csearch_path%3D{schema}");
    let result = tokio::spawn(async move {
        let db = Database::new(&tlab::sqlxdb::Config {
            url: scoped,
            max_connections: 4,
        })
        .await
        .unwrap();
        tlab::migration::run(
            &db,
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")),
            &[],
        )
        .await
        .unwrap();
        scenario(db).await;
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(conn.context().backend())
        .await
        .unwrap();
    result.unwrap();
}
fn code(value: &str) -> PermissionCode {
    PermissionCode(value.to_owned())
}
fn new_role(name: &str) -> NewRole {
    NewRole {
        name: name.into(),
        description: "description".into(),
    }
}
fn assert_db_error(error: tlab::Error, state: &str, constraint: &str) {
    let tlab::Error::DbError(sqlx::Error::Database(db)) = error else {
        panic!("expected original database error")
    };
    assert_eq!(db.code().as_deref(), Some(state));
    assert_eq!(db.constraint(), Some(constraint));
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn migrations_repeat_without_resetting_data() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        let catalog = permission::list(&mut conn.context()).await.unwrap();
        assert_eq!(catalog.iter().map(|p| p.code.as_str()).collect::<Vec<_>>(), ["account.create", "rbac.assignment.manage", "rbac.read", "rbac.role.manage"]);
        let created = role::insert(&mut conn.context(), &new_role("preserved")).await.unwrap();
        role_permission::insert(&mut conn.context(), &NewRolePermission { role_id: created.id, permission_code: code("rbac.read") }).await.unwrap();
        let updated = role::advance_version(&mut conn.context(), created.id, 1).await.unwrap().unwrap();
        drop(conn);
        tlab::migration::run(&db, Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations")), &[]).await.unwrap();
        let mut conn = db.conn().await.unwrap();
        assert_eq!(role::find_by_id(&mut conn.context(), created.id).await.unwrap(), Some(updated));
        assert_eq!(permission::list(&mut conn.context()).await.unwrap(), catalog);
        assert_eq!(role_permission::list_for_role(&mut conn.context(), created.id).await.unwrap().len(), 1);
        let indexes: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_indexes WHERE schemaname=current_schema() AND indexname IN ('tlab_rbac_role_permission_permission_idx','tlab_rbac_account_role_role_idx')").fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(indexes, 2);
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn storage_constraints_queries_versions_and_rollback() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO tlab_account (nickname) VALUES ('member') RETURNING id",
        )
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
        assert!(
            list_role_ids(&mut conn.context(), account)
                .await
                .unwrap()
                .as_slice()
                .is_empty()
        );
        let a = role::insert(&mut conn.context(), &new_role("a"))
            .await
            .unwrap();
        let b = role::insert(&mut conn.context(), &new_role("b"))
            .await
            .unwrap();
        assert_db_error(
            role::insert(&mut conn.context(), &new_role("a"))
                .await
                .unwrap_err(),
            "23505",
            "tlab_rbac_role_name_key",
        );
        let unchanged = RoleMetadata {
            name: a.name.clone(),
            description: a.description.clone(),
        };
        assert_eq!(
            role::update_metadata(&mut conn.context(), a.id, 1, &unchanged)
                .await
                .unwrap(),
            Some(a.clone())
        );
        let updated = role::update_metadata(
            &mut conn.context(),
            a.id,
            1,
            &RoleMetadata {
                name: "renamed".into(),
                description: "changed".into(),
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.version, 2);
        assert!(
            role::advance_version(&mut conn.context(), a.id, 1)
                .await
                .unwrap()
                .is_none()
        );
        assert!(!role::delete(&mut conn.context(), a.id, 1).await.unwrap());
        assert_eq!(
            role::list(&mut conn.context(), Some(a.id), 1)
                .await
                .unwrap(),
            vec![b.clone()]
        );
        for (role_id, codes) in [
            (a.id, vec!["rbac.read"]),
            (b.id, vec!["rbac.read", "account.create"]),
        ] {
            for c in codes {
                let link = NewRolePermission {
                    role_id,
                    permission_code: code(c),
                };
                assert!(
                    role_permission::insert(&mut conn.context(), &link)
                        .await
                        .unwrap()
                );
                assert!(
                    !role_permission::insert(&mut conn.context(), &link)
                        .await
                        .unwrap()
                );
            }
            let link = NewAccountRole {
                account_id: account,
                role_id,
            };
            assert!(
                account_role::insert(&mut conn.context(), &link)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                account_role::insert(&mut conn.context(), &link)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(
            account_role::count_for_account(&mut conn.context(), account)
                .await
                .unwrap(),
            2
        );
        let ids = list_role_ids(&mut conn.context(), account).await.unwrap();
        assert_eq!(ids.as_slice(), &[a.id, b.id]);
        let union = resolve_in_context(&mut conn.context(), &ids).await.unwrap();
        assert_eq!(
            union.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
            vec!["account.create", "rbac.read"]
        );
        assert_eq!(
            resolve_in_context(&mut conn.context(), &RoleIds::default())
                .await
                .unwrap()
                .len(),
            0
        );
        assert_db_error(
            account_role::insert(
                &mut conn.context(),
                &NewAccountRole {
                    account_id: account,
                    role_id: RoleId(i64::MAX),
                },
            )
            .await
            .unwrap_err(),
            "23503",
            "tlab_rbac_account_role_role_fk",
        );
        assert_db_error(
            account_role::insert(
                &mut conn.context(),
                &NewAccountRole {
                    account_id: i64::MAX,
                    role_id: a.id,
                },
            )
            .await
            .unwrap_err(),
            "23503",
            "tlab_rbac_account_role_account_fk",
        );
        assert_db_error(
            role::delete(&mut conn.context(), a.id, 2)
                .await
                .unwrap_err(),
            "23001",
            "tlab_rbac_account_role_role_fk",
        );
        drop(conn);

        // A failed replacement must restore both links and the version.
        let mut tx = db.tx().await.unwrap();
        role_permission::delete_for_role(&mut tx.context(), b.id)
            .await
            .unwrap();
        role::advance_version(&mut tx.context(), b.id, 1)
            .await
            .unwrap()
            .unwrap();
        let error = role_permission::insert(
            &mut tx.context(),
            &NewRolePermission {
                role_id: b.id,
                permission_code: code("missing.permission"),
            },
        )
        .await
        .unwrap_err();
        assert_db_error(error, "23503", "tlab_rbac_role_permission_permission_fk");
        tx.rollback().await.unwrap();
        let mut conn = db.conn().await.unwrap();
        assert_eq!(
            role::find_by_id(&mut conn.context(), b.id).await.unwrap(),
            Some(b.clone())
        );
        assert_eq!(
            resolve_in_context(&mut conn.context(), &ids).await.unwrap(),
            union
        );
        assert!(
            account_role::delete(&mut conn.context(), account, a.id)
                .await
                .unwrap()
        );
        assert!(
            !account_role::delete(&mut conn.context(), account, a.id)
                .await
                .unwrap()
        );
        assert!(role::delete(&mut conn.context(), a.id, 2).await.unwrap());
        assert_eq!(
            role_permission::list_for_role(&mut conn.context(), a.id)
                .await
                .unwrap()
                .len(),
            0
        );
        assert!(
            role::find_by_id(&mut conn.context(), a.id)
                .await
                .unwrap()
                .is_none()
        );
        let remaining = list_role_ids(&mut conn.context(), account).await.unwrap();
        assert_eq!(
            resolve_in_context(&mut conn.context(), &remaining)
                .await
                .unwrap(),
            union
        );
        sqlx::query("DELETE FROM tlab_account WHERE id=$1")
            .bind(account)
            .execute(conn.context().backend())
            .await
            .unwrap();
        assert!(
            !account_role::is_role_assigned(&mut conn.context(), b.id)
                .await
                .unwrap()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn row_locks_and_competing_versions() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        let a = role::insert(&mut conn.context(), &new_role("a"))
            .await
            .unwrap();
        let b = role::insert(&mut conn.context(), &new_role("b"))
            .await
            .unwrap();
        drop(conn);
        let mut tx = db.tx().await.unwrap();
        let locked = role::lock_by_ids(&mut tx.context(), &[b.id, a.id, b.id, RoleId(i64::MAX)])
            .await
            .unwrap();
        assert_eq!(
            locked.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![a.id, b.id]
        );
        let mut competing = db.tx().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(competing.context().backend())
            .await
            .unwrap();
        let tlab::Error::DbError(sqlx::Error::Database(error)) =
            role::lock_by_id(&mut competing.context(), a.id)
                .await
                .unwrap_err()
        else {
            panic!("expected lock timeout")
        };
        assert_eq!(error.code().as_deref(), Some("55P03"));
        competing.rollback().await.unwrap();
        tx.commit().await.unwrap();
        let mut one = db.conn().await.unwrap();
        let mut two = db.conn().await.unwrap();
        let mut ctx_one = one.context();
        let mut ctx_two = two.context();
        let (one, two) = tokio::join!(
            role::advance_version(&mut ctx_one, a.id, 1),
            role::advance_version(&mut ctx_two, a.id, 1)
        );
        assert_eq!(
            usize::from(one.unwrap().is_some()) + usize::from(two.unwrap().is_some()),
            1
        );
        assert_eq!(
            role::find_by_id(&mut ctx_one, a.id)
                .await
                .unwrap()
                .unwrap()
                .version,
            2
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn management_rechecks_delegation_versions_and_atomicity() {
    isolated(|db| async move {
        use rbac::slice;
        use std::sync::Arc;
        let db = Arc::new(db);
        let mut conn = db.conn().await.unwrap();
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO tlab_account (nickname) VALUES ('manager') RETURNING id",
        )
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
        let manager = role::insert(&mut conn.context(), &new_role("manager"))
            .await
            .unwrap();
        for permission_code in ["rbac.role.manage", "rbac.read"] {
            role_permission::insert(
                &mut conn.context(),
                &NewRolePermission {
                    role_id: manager.id,
                    permission_code: code(permission_code),
                },
            )
            .await
            .unwrap();
        }
        let target = role::insert(&mut conn.context(), &new_role("target"))
            .await
            .unwrap();
        role_permission::insert(
            &mut conn.context(),
            &NewRolePermission {
                role_id: target.id,
                permission_code: code("account.create"),
            },
        )
        .await
        .unwrap();
        drop(conn);
        let actor = Actor {
            account_id: account,
            role_ids: RoleIds::from_raw(vec![manager.id.get()]),
        };
        let resolver = DbRolePermissionResolver::new(db.clone());
        let set = |codes: &[&str]| PermissionSet::from_codes(codes.iter().map(|s| code(s)));
        assert_eq!(slice::list_permissions::usecase::execute(&db, &actor, &resolver).await.unwrap().len(), 4);
        assert_eq!(slice::list_roles::usecase::execute(&db, &actor, &resolver).await.unwrap().len(), 2);
        let mut conn = db.conn().await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_role (name, description) SELECT 'page-' || n, '' FROM generate_series(1, 101) AS n")
            .execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert_eq!(slice::list_roles::usecase::execute(&db, &actor, &resolver).await.unwrap().len(), 103);
        let (_, permissions) = slice::list_roles::usecase::get(&db, &actor, &resolver, target.id).await.unwrap();
        assert_eq!(permissions, set(&["account.create"]));
        let error = slice::create_role::usecase::execute(&db, &actor, &resolver, new_role("target")).await.unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacConflict);
        let error = slice::replace_role_permissions::usecase::execute(
            &db, &actor, &resolver, manager.id, 1,
            set(&["rbac.read", "rbac.role.manage", "rbac.assignment.manage"]),
        ).await.unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacForbidden);
        let updated = slice::replace_role_permissions::usecase::execute(
            &db,
            &actor,
            &resolver,
            target.id,
            1,
            set(&["account.create", "rbac.read"]),
        )
        .await
        .unwrap();
        assert_eq!(updated.version, 2);
        let error = slice::replace_role_permissions::usecase::execute(
            &db,
            &actor,
            &resolver,
            target.id,
            2,
            set(&["account.create", "rbac.read", "rbac.assignment.manage"]),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacForbidden);
        let updated = slice::replace_role_permissions::usecase::execute(
            &db,
            &actor,
            &resolver,
            target.id,
            2,
            set(&["rbac.read"]),
        )
        .await
        .unwrap();
        assert_eq!(updated.version, 3);
        let error =
            slice::replace_role_permissions::usecase::execute(&db, &actor, &resolver, target.id, 2, set(&[]))
                .await
                .unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacConflict);
        let mut conn = db.conn().await.unwrap();
        assert_eq!(
            role_permission::list_for_role(&mut conn.context(), target.id)
                .await
                .unwrap(),
            set(&["rbac.read"])
        );
        drop(conn);
        let error = slice::replace_role_permissions::usecase::execute(
            &db,
            &actor,
            &resolver,
            target.id,
            3,
            set(&["rbac.read", "unknown"]),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacForbidden);
        let (first, second) = tokio::join!(
            slice::replace_role_permissions::usecase::execute(&db, &actor, &resolver, target.id, 3, set(&[])),
            slice::replace_role_permissions::usecase::execute(&db, &actor, &resolver, target.id, 3, set(&["rbac.role.manage"]))
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        assert_eq!(first.err().or_else(|| second.err()).unwrap().code, app::error::AppErrorCode::RbacConflict);
        let updated = slice::replace_role_permissions::usecase::execute(&db, &actor, &resolver, target.id, 4, set(&["rbac.read"])).await.unwrap();
        assert_eq!(updated.version, 5);
        let mut conn = db.conn().await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_role_permission() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'test failure'; END $$; CREATE TRIGGER reject_role_permission BEFORE INSERT ON tlab_rbac_role_permission FOR EACH ROW EXECUTE FUNCTION reject_role_permission();")
            .execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let error = slice::replace_role_permissions::usecase::execute(&db, &actor, &resolver, target.id, 5, set(&["rbac.role.manage"])).await.unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::InternalError);
        let mut conn = db.conn().await.unwrap();
        let current = role_permission::list_for_role(&mut conn.context(), target.id).await.unwrap();
        assert_eq!(current, set(&["rbac.read"]));
        let stored = role::find_by_id(&mut conn.context(), target.id).await.unwrap().unwrap();
        assert_eq!(stored.version, 5);
        account_role::insert(&mut conn.context(), &NewAccountRole { account_id: account, role_id: target.id }).await.unwrap();
        drop(conn);
        assert_eq!(slice::delete_role::usecase::execute(&db, &actor, &resolver, target.id, 5).await.unwrap_err().code, app::error::AppErrorCode::RbacConflict);
        let mut conn = db.conn().await.unwrap();
        assert_eq!(role_permission::list_for_role(&mut conn.context(), target.id).await.unwrap(), current);
        drop(conn);
        let created = slice::create_role::usecase::execute(&db, &actor, &resolver, new_role("new"))
            .await
            .unwrap();
        assert_eq!(created.version, 1);
        let updated = slice::update_role::usecase::execute(&db, &actor, &resolver, created.id, 1, RoleMetadata { name: "renamed".into(), description: "description".into() }).await.unwrap();
        assert_eq!(updated.version, 2);
        slice::delete_role::usecase::execute(&db, &actor, &resolver, created.id, 2)
            .await
            .unwrap();
        struct StaleResolver;
        #[async_trait::async_trait]
        impl RolePermissionResolver for StaleResolver {
            async fn resolve(&self, _: &RoleIds) -> tlab::Result<PermissionSet> {
                Ok(PermissionSet::from_codes([code("rbac.role.manage")]))
            }
        }
        let mut conn = db.conn().await.unwrap();
        role_permission::delete(&mut conn.context(), manager.id, &code("rbac.role.manage")).await.unwrap();
        drop(conn);
        let error = slice::create_role::usecase::execute(&db, &actor, &StaleResolver, new_role("must-not-exist")).await.unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacForbidden);
        let mut conn = db.conn().await.unwrap();
        assert!(!role::list_all(&mut conn.context()).await.unwrap().iter().any(|role| role.name == "must-not-exist"));
        sqlx::query("UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=CURRENT_TIMESTAMP WHERE id=$1")
            .bind(account).execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let error = slice::create_role::usecase::execute(&db, &actor, &StaleResolver, new_role("withdrawn")).await.unwrap_err();
        assert_eq!(error.code, app::error::AppErrorCode::RbacForbidden);
    })
    .await;
}

//! Step-02/03 executable boundary: compile the actual RBAC and token sources without
//! editing step-07's binary root. These aliases exactly match app::db.
#[path = "../src/auth/token.rs"]
mod token;
mod auth {
    pub(crate) use crate::token;
}
#[path = "rbac/authorization.rs"]
mod authorization;
mod app {
    pub mod db {
        pub type Database = tlab::sqlxdb::Database<sqlx::Postgres>;
        pub type Context<'a> = tlab::sqlxdb::Context<'a, sqlx::Postgres>;
    }
}
#[allow(
    dead_code,
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

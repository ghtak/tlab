//! Step-02 executable boundary: compile the actual storage module without
//! editing step-07's binary root. These aliases exactly match app::db.
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
    PermissionCode::try_from(value).unwrap()
}
fn id(value: i64) -> RoleId {
    RoleId::try_from(value).unwrap()
}
fn new_role(name: &str) -> NewRole {
    NewRole {
        name: name.into(),
        description: "description".into(),
    }
}
fn assert_db_error(error: sqlx::Error, state: &str, constraint: &str) {
    let sqlx::Error::Database(db) = error else {
        panic!("expected database error")
    };
    assert_eq!(db.code().as_deref(), Some(state));
    assert_eq!(db.constraint(), Some(constraint));
}

#[test]
fn validated_types_and_set_contracts() {
    for value in [i64::MIN, -1, 0] {
        assert!(RoleId::try_from(value).is_err());
    }
    assert_eq!(id(i64::MAX).get(), i64::MAX);
    let ids = RoleIds::try_from_raw(vec![3, 1, 3]).unwrap();
    assert_eq!(
        serde_json::to_value(&ids).unwrap(),
        serde_json::json!([1, 3])
    );
    assert_eq!(
        RoleIds::try_from_raw(vec![1; 33]).unwrap().as_slice(),
        &[id(1)]
    );
    for value in [
        "", "A.b", "a.", ".a", "a..b", "a.*", "a-b", " a", "a\n", "한글", "a.1",
    ] {
        assert!(PermissionCode::try_from(value).is_err(), "{value:?}");
    }
    assert!(PermissionCode::try_from("a".repeat(64).as_str()).is_ok());
    assert!(PermissionCode::try_from("a".repeat(65).as_str()).is_err());
    let left = PermissionSet::from_codes(vec![
        code("rbac.read"),
        code("account.create"),
        code("rbac.read"),
    ]);
    let right = PermissionSet::from_codes(vec![code("rbac.read")]);
    assert!(right.is_subset(&left));
    assert!(left.contains(&code("account.create")));
    assert_eq!(left.difference(&right).len(), 1);
    assert_eq!(left.union(&right), left);
    assert_eq!(left.iter().next().unwrap().as_str(), "account.create");
    assert_eq!(
        PermissionSet::from_codes((0..4097).map(|n| code(&format!("p.x{n}")))).len(),
        4097
    );
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn migrations_repeat_without_resetting_data() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        let catalog = permission::list(&mut conn.context()).await.unwrap();
        assert_eq!(catalog.iter().map(|p| p.code.as_str()).collect::<Vec<_>>(), ["account.create", "rbac.assignment.manage", "rbac.read", "rbac.role.manage"]);
        assert!(permission::find_by_code(&mut conn.context(), &code("missing.code")).await.unwrap().is_none());
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
        let roles = role::list(&mut conn.context(), None, 10).await.unwrap();
        assert_eq!(roles.len(), 1); // no administrator role seeded by migration
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn crud_union_versions_and_rollback() {
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
        assert!(
            list_role_ids(&mut conn.context(), i64::MAX)
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
        assert!(matches!(
            role::insert(&mut conn.context(), &new_role("a")).await,
            Err(tlab::Error::DbError(sqlx::Error::Database(_)))
        ));
        let metadata = RoleMetadata {
            name: a.name.clone(),
            description: a.description.clone(),
        };
        assert_eq!(
            role::update_metadata(&mut conn.context(), a.id, 1, &metadata)
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
        assert!(role::list(&mut conn.context(), None, 0).await.is_err());
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
            assert_eq!(
                account_role::insert(&mut conn.context(), &link)
                    .await
                    .unwrap()
                    .unwrap()
                    .account_id,
                account
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
        assert!(
            account_role::is_role_assigned(&mut conn.context(), a.id)
                .await
                .unwrap()
        );
        let ids = list_role_ids(&mut conn.context(), account).await.unwrap();
        assert_eq!(ids.as_slice(), &[a.id, b.id]);
        let union = resolve_in_context(
            &mut conn.context(),
            &RoleIds::try_from_raw(vec![a.id.get(), b.id.get(), b.id.get(), i64::MAX]).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(union.len(), 2);
        assert_eq!(
            resolve_in_context(&mut conn.context(), &RoleIds::default())
                .await
                .unwrap()
                .len(),
            0
        );
        assert!(matches!(
            role::delete(&mut conn.context(), a.id, 2).await,
            Err(tlab::Error::DbError(sqlx::Error::Database(_)))
        ));
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
        let current_ids = list_role_ids(&mut conn.context(), account).await.unwrap();
        assert_eq!(
            resolve_in_context(&mut conn.context(), &current_ids)
                .await
                .unwrap(),
            union
        );
        drop(conn);
        let mut tx = db.tx().await.unwrap();
        role_permission::delete_for_role(&mut tx.context(), b.id)
            .await
            .unwrap();
        role_permission::insert(
            &mut tx.context(),
            &NewRolePermission {
                role_id: b.id,
                permission_code: code("rbac.role.manage"),
            },
        )
        .await
        .unwrap();
        role::advance_version(&mut tx.context(), b.id, 1)
            .await
            .unwrap()
            .unwrap();
        account_role::delete(&mut tx.context(), account, b.id)
            .await
            .unwrap();
        let failed = role_permission::insert(
            &mut tx.context(),
            &NewRolePermission {
                role_id: b.id,
                permission_code: code("missing.permission"),
            },
        )
        .await;
        assert!(matches!(failed, Err(tlab::Error::DbError(_))));
        tx.rollback().await.unwrap();
        let mut conn = db.conn().await.unwrap();
        assert_eq!(
            role::find_by_id(&mut conn.context(), b.id).await.unwrap(),
            Some(b.clone())
        );
        let current_ids = list_role_ids(&mut conn.context(), account).await.unwrap();
        assert_eq!(
            resolve_in_context(&mut conn.context(), &current_ids)
                .await
                .unwrap(),
            union
        );
        assert!(
            role_permission::delete(&mut conn.context(), a.id, &code("rbac.read"))
                .await
                .unwrap()
        );
        assert!(
            !role_permission::delete(&mut conn.context(), a.id, &code("rbac.read"))
                .await
                .unwrap()
        );
        role_permission::insert(
            &mut conn.context(),
            &NewRolePermission {
                role_id: a.id,
                permission_code: code("rbac.read"),
            },
        )
        .await
        .unwrap();
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
async fn constraints_reject_invalid_links_and_duplicates() {
    isolated(|db| async move {
        let mut conn=db.conn().await.unwrap();
        let r=role::insert(&mut conn.context(),&new_role("r")).await.unwrap();
        for (sql,state,constraint) in [
            ("INSERT INTO tlab_rbac_permission VALUES ('Invalid.*','bad')","23514","tlab_rbac_permission_code_check"),
            ("INSERT INTO tlab_rbac_role (name,description) VALUES ('   ','bad')","23514","tlab_rbac_role_name_check"),
            ("INSERT INTO tlab_rbac_account_role VALUES (9223372036854775807,9223372036854775807,now())","23503","tlab_rbac_account_role_account_fk"),
            ("INSERT INTO tlab_rbac_role_permission VALUES (9223372036854775807,'rbac.read')","23503","tlab_rbac_role_permission_role_fk"),
        ] {
            let error=sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(conn.context().backend()).await.unwrap_err();
            assert_db_error(error,state,constraint);
        }
        let error=sqlx::query("INSERT INTO tlab_rbac_role_permission VALUES ($1,'unknown.permission')").bind(r.id.get()).execute(conn.context().backend()).await.unwrap_err();
        assert_db_error(error,"23503","tlab_rbac_role_permission_permission_fk");
        role_permission::insert(&mut conn.context(),&NewRolePermission {role_id:r.id,permission_code:code("rbac.read")}).await.unwrap();
        let error=sqlx::query("INSERT INTO tlab_rbac_role_permission VALUES ($1,'rbac.read')").bind(r.id.get()).execute(conn.context().backend()).await.unwrap_err();
        assert_db_error(error,"23505","tlab_rbac_role_permission_pkey");
        let error=sqlx::query("DELETE FROM tlab_rbac_permission WHERE code='rbac.read'").execute(conn.context().backend()).await.unwrap_err();
        assert_db_error(error,"23001","tlab_rbac_role_permission_permission_fk");
        let account:i64=sqlx::query_scalar("INSERT INTO tlab_account (nickname) VALUES ('member') RETURNING id").fetch_one(conn.context().backend()).await.unwrap();
        let missing=account_role::insert(&mut conn.context(),&NewAccountRole {account_id:account,role_id:id(i64::MAX)}).await.unwrap_err();
        assert!(matches!(missing,tlab::Error::DbError(_))); // the original foreign key error is preserved
        account_role::insert(&mut conn.context(),&NewAccountRole {account_id:account,role_id:r.id}).await.unwrap();
        let error=sqlx::query("INSERT INTO tlab_rbac_account_role (account_id,role_id) VALUES ($1,$2)").bind(account).bind(r.id.get()).execute(conn.context().backend()).await.unwrap_err();
        assert_db_error(error,"23505","tlab_rbac_account_role_pkey");
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn full_lookup_and_corrupt_storage_fail_closed() {
    isolated(|db| async move {
        let mut conn=db.conn().await.unwrap();
        let r=role::insert(&mut conn.context(),&new_role("oversized")).await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_permission SELECT 'p.x'||n, 'test' FROM generate_series(1,129) n").execute(conn.context().backend()).await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_role_permission SELECT $1,code FROM tlab_rbac_permission WHERE code LIKE 'p.%'").bind(r.id.get()).execute(conn.context().backend()).await.unwrap();
        assert_eq!(role_permission::list_for_role(&mut conn.context(),r.id).await.unwrap().len(),129);
        assert_eq!(resolve_in_context(&mut conn.context(),&RoleIds::try_from_raw(vec![r.id.get()]).unwrap()).await.unwrap().len(),129);
        role_permission::delete(&mut conn.context(),r.id,&code("p.x129")).await.unwrap();
        assert_eq!(role_permission::list_for_role(&mut conn.context(),r.id).await.unwrap().len(),128);
        let account:i64=sqlx::query_scalar("INSERT INTO tlab_account (nickname) VALUES ('member') RETURNING id").fetch_one(conn.context().backend()).await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_role (name,description) SELECT 'extra'||n,'' FROM generate_series(1,32) n").execute(conn.context().backend()).await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_account_role (account_id,role_id) SELECT $1,id FROM tlab_rbac_role").bind(account).execute(conn.context().backend()).await.unwrap();
        assert_eq!(list_role_ids(&mut conn.context(),account).await.unwrap().as_slice().len(),33);
        sqlx::query("ALTER TABLE tlab_rbac_permission DROP CONSTRAINT tlab_rbac_permission_code_check").execute(conn.context().backend()).await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_permission VALUES ('INVALID','corrupt')").execute(conn.context().backend()).await.unwrap();
        assert!(matches!(permission::list(&mut conn.context()).await,Err(tlab::Error::IllegalState(_))));
        assert_eq!(role_permission::delete_for_role(&mut conn.context(),r.id).await.unwrap(),128);
        sqlx::query("INSERT INTO tlab_rbac_role_permission VALUES ($1,'INVALID')").bind(r.id.get()).execute(conn.context().backend()).await.unwrap();
        assert!(matches!(resolve_in_context(&mut conn.context(),&RoleIds::try_from_raw(vec![r.id.get()]).unwrap()).await,Err(tlab::Error::IllegalState(_))));
        sqlx::query("DROP TABLE tlab_rbac_role_permission").execute(conn.context().backend()).await.unwrap();
        assert!(matches!(resolve_in_context(&mut conn.context(),&RoleIds::try_from_raw(vec![r.id.get()]).unwrap()).await,Err(tlab::Error::DbError(_))));
    }).await;
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
        let locked = role::lock_by_ids(&mut tx.context(), &[b.id, a.id, b.id, id(i64::MAX)])
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
        let error = role::lock_by_id(&mut competing.context(), a.id)
            .await
            .unwrap_err();
        let tlab::Error::DbError(sqlx::Error::Database(error)) = error else {
            panic!("expected lock timeout")
        };
        assert_eq!(error.code().as_deref(), Some("55P03"));
        competing.rollback().await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = db.tx().await.unwrap();
        assert!(
            role::lock_by_id(&mut tx.context(), a.id)
                .await
                .unwrap()
                .is_some()
        );
        tx.rollback().await.unwrap();
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

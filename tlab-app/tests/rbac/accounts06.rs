use super::{app, auth, auth_create_local_user, code, isolated, new_role, rbac::*};
use app::{db::Database, error::AppErrorCode};
use repository::{account_role, permission, role, role_permission};
use std::{sync::Arc, time::Duration};
use tokio::{sync::Semaphore, time::timeout};

async fn account(db: &Database, nickname: &str) -> i64 {
    let mut conn = db.conn().await.unwrap();
    sqlx::query_scalar("INSERT INTO tlab_account (nickname) VALUES ($1) RETURNING id")
        .bind(nickname)
        .fetch_one(conn.context().backend())
        .await
        .unwrap()
}
async fn configured_role(db: &Database, name: &str, codes: &[&str]) -> RoleId {
    let mut conn = db.conn().await.unwrap();
    let created = role::insert(&mut conn.context(), &new_role(name))
        .await
        .unwrap();
    for value in codes {
        role_permission::insert(
            &mut conn.context(),
            &NewRolePermission {
                role_id: created.id,
                permission_code: code(value),
            },
        )
        .await
        .unwrap();
    }
    created.id
}
async fn assign(db: &Database, account_id: i64, role_id: RoleId) {
    let mut conn = db.conn().await.unwrap();
    account_role::insert(
        &mut conn.context(),
        &NewAccountRole {
            account_id,
            role_id,
        },
    )
    .await
    .unwrap();
}
async fn ids(db: &Database, account_id: i64) -> RoleIds {
    let mut conn = db.conn().await.unwrap();
    list_role_ids(&mut conn.context(), account_id)
        .await
        .unwrap()
}
async fn withdraw(db: &Database, id: i64) {
    let mut conn = db.conn().await.unwrap();
    sqlx::query("UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=now() WHERE id=$1")
        .bind(id)
        .execute(conn.context().backend())
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn assignment_policy_reads_idempotency_and_account_states() {
    isolated(|db| async move {
        let db = Arc::new(db);
        let operator = account(&db, "operator").await;
        let target = account(&db, "target").await;
        let manage =
            configured_role(&db, "manager", &["rbac.assignment.manage", "rbac.read"]).await;
        let read = configured_role(&db, "reader", &["rbac.read"]).await;
        let read2 = configured_role(&db, "reader2", &["rbac.read"]).await;
        let strong = configured_role(&db, "strong", &["account.create"]).await;
        assign(&db, operator, manage).await;
        let actor = Actor {
            account_id: operator,
            role_ids: RoleIds::from_raw(vec![manage.get()]),
        };
        let resolver = DbRolePermissionResolver::new(db.clone());
        let self_only = Actor {
            account_id: target,
            role_ids: RoleIds::default(),
        };
        assert_eq!(
            slice::list_account_roles::usecase::execute(&db, &self_only, &resolver, target)
                .await
                .unwrap(),
            RoleIds::default()
        );
        assert_eq!(
            slice::get_effective_permissions::usecase::execute(
                &db, &self_only, &resolver, operator
            )
            .await
            .unwrap_err()
            .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(
            slice::grant_account_role::usecase::execute(&db, &self_only, &resolver, target, read)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(
            slice::revoke_account_role::usecase::execute(&db, &self_only, &resolver, target, read)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, strong)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(ids(&db, target).await, RoleIds::default());
        for _ in 0..2 {
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, read)
                .await
                .unwrap();
        }
        let mut conn = db.conn().await.unwrap();
        let granted: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT granted_at FROM tlab_rbac_account_role WHERE account_id=$1 AND role_id=$2",
        )
        .bind(target)
        .bind(read.get())
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
        drop(conn);
        slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, read)
            .await
            .unwrap();
        let mut conn = db.conn().await.unwrap();
        let unchanged: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT granted_at FROM tlab_rbac_account_role WHERE account_id=$1 AND role_id=$2",
        )
        .bind(target)
        .bind(read.get())
        .fetch_one(conn.context().backend())
        .await
        .unwrap();
        assert_eq!(granted, unchanged);
        drop(conn);
        assign(&db, target, read2).await;
        assign(&db, target, strong).await;
        // A token with no roles can display newly granted roles from the DB.
        assert_eq!(
            slice::list_account_roles::usecase::execute(&db, &self_only, &resolver, target)
                .await
                .unwrap()
                .as_slice(),
            &[read, read2, strong]
        );
        assert_eq!(
            slice::get_effective_permissions::usecase::execute(&db, &actor, &resolver, target)
                .await
                .unwrap()
                .len(),
            2
        );
        for id in [strong, strong, read, read] {
            slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, target, id)
                .await
                .unwrap();
        }
        assert_eq!(
            slice::get_effective_permissions::usecase::execute(&db, &self_only, &resolver, target)
                .await
                .unwrap(),
            PermissionSet::from_codes([code("rbac.read")])
        );
        for missing_account in [false, true] {
            let a = if missing_account { i64::MAX } else { target };
            let r = if missing_account {
                read
            } else {
                RoleId(i64::MAX)
            };
            assert_eq!(
                slice::grant_account_role::usecase::execute(&db, &actor, &resolver, a, r)
                    .await
                    .unwrap_err()
                    .code,
                AppErrorCode::RbacNotFound
            );
            assert_eq!(
                slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, a, r)
                    .await
                    .unwrap_err()
                    .code,
                AppErrorCode::RbacNotFound
            );
        }
        assert_eq!(
            slice::list_account_roles::usecase::execute(&db, &actor, &resolver, i64::MAX)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacNotFound
        );
        withdraw(&db, target).await;
        assert_eq!(
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, read)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::AccountUnavailable
        );
        slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, target, read2)
            .await
            .unwrap();
        // No self/last-manager protection, and this old Access keeps its role IDs.
        slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, operator, manage)
            .await
            .unwrap();
        assert_eq!(ids(&db, operator).await, RoleIds::default());
        slice::grant_account_role::usecase::execute(&db, &actor, &resolver, operator, manage)
            .await
            .unwrap();
        withdraw(&db, operator).await;
        // Management permissions remain present: status is the sole denial.
        assert_eq!(
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, operator, read)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(
            slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, operator, manage)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(ids(&db, operator).await.as_slice(), &[manage]);
    })
    .await;
}

// Observe a real PostgreSQL lock wait, without timing-based assumptions.
async fn wait_for_blocker(db: &Database, blocker: i32) {
    timeout(Duration::from_secs(10), async {
        let mut conn = db.conn().await.unwrap();
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid))",
            )
            .bind(blocker)
            .fetch_one(conn.context().backend())
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker must reach its row lock");
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn assignment_waits_for_role_edits_and_deletion_then_rechecks() {
    isolated(|db| async move {
        let db = Arc::new(db);
        let operator = account(&db, "operator").await;
        let target = account(&db, "target").await;
        let manage =
            configured_role(&db, "manager", &["rbac.assignment.manage", "rbac.read"]).await;
        let read = configured_role(&db, "reader", &["rbac.read"]).await;
        // Each worker uses a real DB resolver at entry. The other transaction
        // holds roles, not the actor account, so the role wait is observable.
        for change in [
            "target_gains_permission",
            "actor_loses_permission",
            "role_deleted",
        ] {
            let mut edit = db.tx().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(edit.context().backend())
                .await
                .unwrap();
            let editing = if change == "actor_loses_permission" {
                manage
            } else {
                read
            };
            role::lock_by_id(&mut edit.context(), editing)
                .await
                .unwrap()
                .unwrap();
            let task_db = db.clone();
            let worker = tokio::spawn(async move {
                let actor = Actor {
                    account_id: operator,
                    role_ids: RoleIds::from_raw(vec![manage.get()]),
                };
                let resolver = DbRolePermissionResolver::new(task_db.clone());
                slice::grant_account_role::usecase::execute(
                    &task_db, &actor, &resolver, target, read,
                )
                .await
            });
            wait_for_blocker(&db, blocker).await;
            match change {
                "target_gains_permission" => {
                    role_permission::insert(
                        &mut edit.context(),
                        &NewRolePermission {
                            role_id: read,
                            permission_code: code("account.create"),
                        },
                    )
                    .await
                    .unwrap();
                }
                "actor_loses_permission" => {
                    role_permission::delete(
                        &mut edit.context(),
                        manage,
                        &code("rbac.assignment.manage"),
                    )
                    .await
                    .unwrap();
                }
                _ => {
                    role::delete(&mut edit.context(), read, 1).await.unwrap();
                }
            }
            edit.commit().await.unwrap();
            let error = timeout(Duration::from_secs(10), worker)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(
                error.code,
                if change == "role_deleted" {
                    AppErrorCode::RbacNotFound
                } else {
                    AppErrorCode::RbacForbidden
                },
                "{change}"
            );
            assert_eq!(ids(&db, target).await, RoleIds::default());
            if change == "target_gains_permission" {
                let mut conn = db.conn().await.unwrap();
                role_permission::delete(&mut conn.context(), read, &code("account.create"))
                    .await
                    .unwrap();
            }
            if change == "actor_loses_permission" {
                let mut conn = db.conn().await.unwrap();
                role_permission::insert(
                    &mut conn.context(),
                    &NewRolePermission {
                        role_id: manage,
                        permission_code: code("rbac.assignment.manage"),
                    },
                )
                .await
                .unwrap();
            }
        }
    })
    .await;
}

struct TestHasher;
impl tlab::password::PasswordHasher for TestHasher {
    fn hash(&self, _: &str) -> tlab::Result<String> {
        Ok("test-only-hash".into())
    }
    fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
        unreachable!()
    }
}
fn input(email: &str) -> auth::LocalAccountInput {
    auth::LocalAccountInput {
        nickname: " admin ".into(),
        email: email.into(),
        password: " password with spaces ".into(),
    }
}
async fn bootstrap(
    db: &Database,
    email: &str,
) -> app::error::AppResult<slice::bootstrap_admin::usecase::CreatedAdmin> {
    slice::bootstrap_admin::usecase::execute(
        db,
        &auth::passwd_policy::NonEmptyPasswordPolicy,
        Arc::new(TestHasher),
        Arc::new(Semaphore::new(2)),
        input(email),
    )
    .await
}
async fn counts(db: &Database) -> (i64, i64, i64, i64, i64) {
    let mut conn = db.conn().await.unwrap();
    sqlx::query_as("SELECT (SELECT count(*) FROM tlab_account), (SELECT count(*) FROM tlab_account_identity), (SELECT count(*) FROM tlab_rbac_role), (SELECT count(*) FROM tlab_rbac_role_permission), (SELECT count(*) FROM tlab_rbac_account_role)")
        .fetch_one(conn.context().backend()).await.unwrap()
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn bootstrap_catalog_conflicts_and_atomicity() {
    isolated(|db| async move {
        let mut conn = db.conn().await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_permission (code, description) VALUES ('extra.permission','test')")
            .execute(conn.context().backend()).await.unwrap();
        // Fail the final write to exercise rollback of account, identity, role
        // and all permission links through the real bootstrap usecase.
        sqlx::raw_sql("CREATE FUNCTION reject_assignment() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic assignment failure'; END $$; CREATE TRIGGER reject_assignment BEFORE INSERT ON tlab_rbac_account_role FOR EACH ROW EXECUTE FUNCTION reject_assignment()")
            .execute(conn.context().backend()).await.unwrap();
        drop(conn);
        assert_eq!(bootstrap(&db, "admin@example.com").await.unwrap_err().code, AppErrorCode::InternalError);
        assert_eq!(counts(&db).await, (0,0,0,0,0));
        let mut conn = db.conn().await.unwrap();
        sqlx::query("DROP TRIGGER reject_assignment ON tlab_rbac_account_role").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let created = bootstrap(&db, "one@example.com").await.unwrap();
        assert_eq!(counts(&db).await, (1,1,1,5,1));
        assert_eq!(ids(&db, created.account_id).await.as_slice(), &[created.role_id]);
        let mut conn = db.conn().await.unwrap();
        let stored: (String,String) = sqlx::query_as("SELECT nickname,status FROM tlab_account WHERE id=$1")
            .bind(created.account_id).fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(stored, ("admin".into(), "REGISTERED".into()));
        assert_eq!(role_permission::list_for_role(&mut conn.context(), created.role_id).await.unwrap().len(), permission::list(&mut conn.context()).await.unwrap().len());
        drop(conn);
        assert_eq!(bootstrap(&db, "third@example.com").await.unwrap_err().code, AppErrorCode::RbacConflict);
        assert_eq!(counts(&db).await, (1,1,1,5,1));
    }).await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn shared_signup_preserves_email_retry_and_bootstrap_duplicate_conflict() {
    isolated(|db| async move {
        let created = auth_create_local_user::execute(
            &db,
            &auth::passwd_policy::NonEmptyPasswordPolicy,
            Arc::new(TestHasher),
            Arc::new(Semaphore::new(2)),
            input(" Member@Example.com "),
        )
        .await
        .unwrap();
        assert_eq!(created.status, auth::entity::AccountStatus::Registered);
        assert_eq!(counts(&db).await, (1, 1, 0, 0, 0));
        assert_eq!(
            bootstrap(&db, "member@example.com").await.unwrap_err().code,
            AppErrorCode::EmailAlreadyExists
        );
        assert_eq!(counts(&db).await, (1, 1, 0, 0, 0));
        let (one, two) = tokio::join!(
            auth_create_local_user::execute(
                &db,
                &auth::passwd_policy::NonEmptyPasswordPolicy,
                Arc::new(TestHasher),
                Arc::new(Semaphore::new(2)),
                input("race@example.com")
            ),
            auth_create_local_user::execute(
                &db,
                &auth::passwd_policy::NonEmptyPasswordPolicy,
                Arc::new(TestHasher),
                Arc::new(Semaphore::new(2)),
                input("RACE@example.com")
            )
        );
        assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
        assert_eq!(counts(&db).await, (2, 2, 0, 0, 0));
        let retry = auth_create_local_user::execute(
            &db,
            &auth::passwd_policy::NonEmptyPasswordPolicy,
            Arc::new(TestHasher),
            Arc::new(Semaphore::new(2)),
            input("race@example.com"),
        )
        .await
        .unwrap_err();
        assert_eq!(retry.code, AppErrorCode::EmailAlreadyExists);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn assignment_reads_accounts_without_waiting_for_status_updates() {
    isolated(|db| async move {
        let db = Arc::new(db);
        let operator = account(&db, "operator").await;
        let target = account(&db, "target").await;
        let manage = configured_role(&db, "manager", &["rbac.assignment.manage"]).await;
        let empty = configured_role(&db, "empty", &[]).await;
        let actor = Actor {
            account_id: operator,
            role_ids: RoleIds::from_raw(vec![manage.get()]),
        };
        let resolver = DbRolePermissionResolver::new(db.clone());
        let mut held = db.tx().await.unwrap();
        sqlx::query(
            "UPDATE tlab_account SET status='WITHDRAWN', withdrawn_at=now() WHERE id=ANY($1)",
        )
        .bind(vec![operator, target])
        .execute(held.context().backend())
        .await
        .unwrap();
        timeout(Duration::from_secs(2), async {
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, empty)
                .await
                .unwrap();
            assert_eq!(ids(&db, target).await.as_slice(), &[empty]);
            slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, target, empty)
                .await
                .unwrap();
            assert_eq!(ids(&db, target).await, RoleIds::default());
        })
        .await
        .expect("assignment must not wait for account status updates");
        held.commit().await.unwrap();
        assert_eq!(
            slice::grant_account_role::usecase::execute(&db, &actor, &resolver, target, empty)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
        assert_eq!(
            slice::revoke_account_role::usecase::execute(&db, &actor, &resolver, target, empty)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacForbidden
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn bootstrap_concurrent_signup_email_collision_rolls_back_auth_rows() {
    isolated(|db| async move {
        use std::sync::{Condvar, Mutex};
        struct Gate {
            released: Mutex<bool>,
            wake: Condvar,
            started: tokio::sync::mpsc::UnboundedSender<()>,
        }
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.released.lock().unwrap() = true;
                self.0.wake.notify_all();
            }
        }
        struct Release(Arc<Gate>);
        struct GatedHasher(Arc<Gate>);
        impl tlab::password::PasswordHasher for GatedHasher {
            fn hash(&self, _: &str) -> tlab::Result<String> {
                self.0.started.send(()).unwrap();
                let mut released = self.0.released.lock().unwrap();
                while !*released {
                    released = self.0.wake.wait(released).unwrap();
                }
                Ok("test-only-hash".into())
            }
            fn verify(&self, _: &str, _: &str) -> tlab::Result<bool> {
                unreachable!()
            }
        }
        let db = Arc::new(db);
        let (started, mut events) = tokio::sync::mpsc::unbounded_channel();
        let gate = Arc::new(Gate {
            released: Mutex::new(false),
            wake: Condvar::new(),
            started,
        });
        let release = Release(gate.clone());
        let task_db = db.clone();
        let worker = tokio::spawn(async move {
            slice::bootstrap_admin::usecase::execute(
                &task_db,
                &auth::passwd_policy::NonEmptyPasswordPolicy,
                Arc::new(GatedHasher(gate)),
                Arc::new(Semaphore::new(1)),
                input("race@example.com"),
            )
            .await
        });
        timeout(Duration::from_secs(10), events.recv())
            .await
            .unwrap()
            .unwrap();
        // While bootstrap is hashing, no connection or transaction is held:
        // all four pool connections remain available to the caller.
        let mut available = Vec::new();
        for _ in 0..4 {
            available.push(
                timeout(Duration::from_secs(2), db.conn())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        drop(available);
        auth_create_local_user::execute(
            &db,
            &auth::passwd_policy::NonEmptyPasswordPolicy,
            Arc::new(TestHasher),
            Arc::new(Semaphore::new(1)),
            input("RACE@example.com"),
        )
        .await
        .unwrap();
        drop(release);
        assert_eq!(
            timeout(Duration::from_secs(10), worker)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            AppErrorCode::EmailAlreadyExists
        );
        assert_eq!(counts(&db).await, (1, 1, 0, 0, 0));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn effective_permissions_missing_account_returns_not_found() {
    isolated(|db| async move {
        let db = Arc::new(db);
        let operator = account(&db, "reader").await;
        let read = configured_role(&db, "read", &["rbac.read"]).await;
        let actor = Actor {
            account_id: operator,
            role_ids: RoleIds::from_raw(vec![read.get()]),
        };
        let resolver = DbRolePermissionResolver::new(db.clone());
        assert_eq!(
            slice::get_effective_permissions::usecase::execute(&db, &actor, &resolver, i64::MAX)
                .await
                .unwrap_err()
                .code,
            AppErrorCode::RbacNotFound
        );
    })
    .await;
}

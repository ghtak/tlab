mod support;

use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};
type Database = tlab::sqlxdb::Database<sqlx::Postgres>;

async fn counts(db: &Database) -> (i64, i64, i64, i64, i64) {
    let mut conn = db.conn().await.unwrap();
    sqlx::query_as("SELECT (SELECT count(*) FROM tlab_account), (SELECT count(*) FROM tlab_account_identity), (SELECT count(*) FROM tlab_rbac_role), (SELECT count(*) FROM tlab_rbac_role_permission), (SELECT count(*) FROM tlab_rbac_account_role)").fetch_one(conn.context().backend()).await.unwrap()
}

async fn run(url: String, email: &str, password: &str) -> std::process::Output {
    let email = email.to_owned();
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        let dir = tempfile::tempdir().unwrap();
        // Intentionally missing TLS/JWT files: CLI must not initialize servers,
        // signing keys or tracing files. Current directory has no .env.
        let mut child = Command::new(env!("CARGO_BIN_EXE_tlab-app"))
            .current_dir(dir.path())
            .env(
                "TLAB_CONFIG_FILE",
                concat!(env!("CARGO_MANIFEST_DIR"), "/config/app.toml"),
            )
            .env("TLAB_APP_DATABASE__URL", url)
            .env("TLAB_APP_AUTH__JWT__GENERATE_IF_MISSING", "false")
            .env("TLAB_APP_SERVER__HTTPS__TLS__GENERATE_IF_MISSING", "false")
            .args([
                "bootstrap-admin",
                "--email",
                &email,
                "--nickname",
                "operator",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(password.as_bytes()).unwrap();
        input.write_all(b"\n").unwrap();
        drop(input);
        let output = child.wait_with_output().unwrap();
        for stream in [&output.stdout, &output.stderr] {
            let text = String::from_utf8_lossy(stream);
            assert!(!text.contains(&password));
            assert!(!text.contains("$argon2"));
            assert!(!text.contains("postgres://"));
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        output
    })
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires PostgreSQL and TLAB_TEST_DATABASE_URL"]
async fn bootstrap_process_atomicity_success_conflicts_and_secret_output() {
    support::isolated_with_url(|db,url| async move {
        let mut conn = db.conn().await.unwrap();
        sqlx::query("INSERT INTO tlab_rbac_permission (code,description) VALUES ('extra.permission','test')").execute(conn.context().backend()).await.unwrap();
        sqlx::raw_sql("CREATE FUNCTION reject_bootstrap() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic failure'; END $$; CREATE TRIGGER reject_bootstrap BEFORE INSERT ON tlab_rbac_account_role FOR EACH ROW EXECUTE FUNCTION reject_bootstrap()").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let failed = run(url.clone(),"admin@example.test"," secret stdin password ").await;
        assert!(!failed.status.success());
        assert!(failed.stdout.is_empty());
        assert_eq!(serde_json::from_slice::<Value>(&failed.stderr).unwrap()["code"],"INTERNAL_ERROR");
        assert_eq!(counts(&db).await,(0,0,0,0,0));
        let mut conn = db.conn().await.unwrap();
        sqlx::query("DROP TRIGGER reject_bootstrap ON tlab_rbac_account_role").execute(conn.context().backend()).await.unwrap();
        drop(conn);
        let success = run(url.clone(),"admin@example.test"," secret stdin password ").await;
        assert!(success.status.success());
        assert!(success.stderr.is_empty());
        let created: Value = serde_json::from_slice(&success.stdout).unwrap();
        assert_eq!(created.as_object().unwrap().len(),2);
        let id = created["account_id"].as_i64().unwrap();
        let role = created["role_id"].as_i64().unwrap();
        assert_eq!(counts(&db).await,(1,1,1,5,1));
        let mut conn = db.conn().await.unwrap();
        let (hash, assigned_role): (String,i64) = sqlx::query_as("SELECT i.password_hash,ar.role_id FROM tlab_account_identity i JOIN tlab_rbac_account_role ar ON ar.account_id=i.account_id WHERE i.account_id=$1").bind(id).fetch_one(conn.context().backend()).await.unwrap();
        assert_eq!(assigned_role,role);
        use tlab::password::PasswordHasher;
        let hasher = tlab::password::argon2id::Hasher::new(&Default::default()).unwrap();
        assert!(hasher.verify(" secret stdin password ",&hash).unwrap());
        assert!(!hasher.verify("secret stdin password",&hash).unwrap());
        drop(conn);
        for (email,code) in [("another@example.test","RBAC_CONFLICT"),("ADMIN@example.test","EMAIL_ALREADY_EXISTS")] {
            let conflict = run(url.clone(),email,"second secret").await;
            assert!(!conflict.status.success());
            assert!(conflict.stdout.is_empty());
            assert_eq!(serde_json::from_slice::<Value>(&conflict.stderr).unwrap()["code"],code);
            assert_eq!(counts(&db).await,(1,1,1,5,1));
        }
    }).await;
}

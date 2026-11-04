# tlab

공통 기반 기능을 제공하는 Rust 라이브러리.

SQLx 기반 풀·연결·트랜잭션은 `tlab::sqlxdb`에서 제공한다. 최초 지원 대상은
Tokio·PostgreSQL이며 쿼리는 SQLx 0.9 API를 사용한다.
공개 계약과 검증 결과는 [DB 설계](docs/specs/database/plan.md)를 참조한다.

`tlab-app`은 기본적으로 `127.0.0.1:8080`에서 HTTP를 실행한다. 별도 터미널에서
`just db-up postgres`로 DB를 시작하고 `.env.example`을 `.env`로 복사한 다음
`just app-run` → `just app-health`로 생존 확인 응답 `ok`·`HTTP 200`을 확인하고 Ctrl+C로 종료한다.
앱은 실행 디렉터리의 `.env`를 읽으며 기존 환경변수가 우선한다. PostgreSQL 포트를 변경하면 `.env`의 URL도 변경한다.
HTTPS·설정 변경과 검증 도구의 역할은 [HTTP 앱 도구](docs/verification/tools/http/README.md)를 참조한다.

DB 마이그레이션은 앱 설정의 DB에 다음 명령으로 실행한다. 서버는 기동하지 않는다.

```sh
cargo run -p tlab-app -- --migrate tlab-app/migrations
```

디렉터리에는 `0001_create.sql`처럼 번호와 이름을 붙인 SQL 파일을 둔다. 코드 작업은
`tlab-app/src/migrations.rs`의 `CODE`에 번호와 함수를 등록한다. 함수는 다음과 같은
형태로 DB 문맥을 사용한다.

```rust,ignore
fn seed<'a, 'c>(ctx: &'a mut tlab::sqlxdb::Context<'c, sqlx::Postgres>)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = tlab::Result<()>> + Send + 'a>>
{
    Box::pin(async move {
        sqlx::query("SELECT 1").execute(ctx.backend()).await?;
        Ok(())
    })
}
// CodeMigration { sequence: 10, name: "seed", run: seed }
```

SQL과 코드 작업은 번호 순서로 매번 실행한다. 이력·롤백·동시 실행 잠금은 없으므로
사용자가 작업의 반복 안전성, 실행 직렬화 및 실패 후 복구를 책임진다. 파일 규칙과
실패 처리의 상세는 [마이그레이션 명세](docs/specs/database/migrate/spec.md)를 참조한다.

로컬 테스트용 PostgreSQL·Valkey·Oracle Free 환경은 [DB 검증 도구](docs/verification/tools/database/README.md)를 참조한다.
Docker·Compose·just를 준비한 뒤 프로젝트 루트에서 실행한다.

```sh
just db-up
just db-check
just db-down
```

DB 통합 테스트는 환경을 가동한 상태에서 명시적으로 실행한다. URL이 없거나
접속에 실패하면 테스트가 실패한다. 일반 `cargo test --workspace --locked`에서는
외부 DB를 사용하는 테스트를 제외한다.

```sh
TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --locked --test sqlxdb -- --ignored
TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --locked --test migration -- --ignored
```

위 URL은 공개된 로컬 개발용 기본값이다. 포트를 변경했다면 테스트 URL에도 반영한다.

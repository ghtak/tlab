# tlab

공통 기반 기능을 제공하는 Rust 라이브러리.

`tlab-app-ui`는 React 기반 프런트엔드다. Node.js 22.12 이상과 npm을 준비한 뒤 프로젝트 루트에서 최초 한 번 의존성을 설치한다.

```sh
npm --prefix tlab-app-ui ci
```

이후 프로젝트 루트에서 개발 서버를 실행하고, 표시된 주소로 접속한다. 종료하려면 Ctrl+C를 누른다.

```sh
just app-run-ui
```

프로덕션 빌드는 `npm --prefix tlab-app-ui run build`, 빌드 결과 확인은 `npm --prefix tlab-app-ui run preview`를 사용한다.
TanStack Router의 파일 기반 라우트는 `src/routes`에 추가하며, shadcn 컴포넌트는
`components.json` 설정을 사용한다. UI와 Rust 서버는 현재 별도로 실행된다.
상세 구성과 개발 방법은 [UI README](tlab-app-ui/README.md)를 참조한다.

개발 명령은 루트 `justfile`에서 제공하며, 정의는 `just/db.just`,
`just/app.just`, `just/http.just`에 나누어 관리한다. `just --list`로 명령을 확인할 수 있다.

SQLx 기반 풀·연결·트랜잭션은 `tlab::sqlxdb`에서 제공한다. 최초 지원 대상은
Tokio·PostgreSQL이며 쿼리는 SQLx 0.9 API를 사용한다.
공개 계약과 검증 결과는 [DB 설계](docs/specs/database/plan.md)를 참조한다.

`tlab-app`은 기본적으로 `127.0.0.1:8080`에서 HTTP를 실행한다. 별도 터미널에서
`just db-up postgres`로 DB를 시작하고 `.env.example`을 `.env`로 복사한 다음
`just app-run` → `just http-health`로 생존 확인 응답 `ok`·`HTTP 200`을 확인하고 Ctrl+C로 종료한다.
앱은 실행 디렉터리의 `.env`를 읽으며 기존 환경변수가 우선한다. 개발 환경에서는 `auth.jwt.generate_if_missing = true`로 지정해 로컬 키 쌍을 생성하거나 설정된 경로에 Ed25519 키 쌍을 미리 준비한다. 운영에서는 키를 안전하게 제공하고 issuer·audience를 배포 환경에 맞게 설정한다. PostgreSQL 포트를 변경하면 `.env`의 URL도 변경한다.
HTTPS·설정 변경과 검증 도구의 역할은 [HTTP 앱 도구](docs/verification/tools/http/README.md)를 참조한다.

DB 마이그레이션은 앱 설정의 DB에 다음 명령으로 실행한다. 서버는 기동하지 않는다.

```sh
just app-migration
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

계정 migration 적용 후 `POST /api/v1/accounts/local`로 로컬 사용자를 추가할 수 있다.
JSON 입력은 `nickname`, `email`, `password`이며 성공 시 HTTP 201과 계정 ID·REGISTERED 상태를 반환한다.
비밀번호는 Argon2id PHC로 저장한다. 기본 비밀번호 정책은 빈 문자열 거부이며
`auth::passwd_policy::PasswordPolicy` 구현체로 교체할 수 있다.
`[auth] max_concurrent_password_hashes`의 기본값은 2이며, 슬롯이 가득 차면 HTTP 503을 반환한다.
로그인·토큰 갱신과 Bearer 인증 계약은 [JWT API](docs/specs/auth/jwt-api/plan.md)를 따른다. 개별 업무의 권한 판단은 해당 핸들러가 담당한다.
웹 쿠키 로그인은 `POST /web/v1/auth/local`, 보호 요청은 `GET /web/v1/auth/me`, 로그아웃은 `POST /web/v1/auth/logout`이다.
unsafe 웹 요청에는 `[auth.web] origin`과 일치하는 Origin이 필요하다. 기본은 `https://localhost:8081`이며
Secure·HttpOnly 쿠키를 사용한다. Access가 없거나 정상 만료되면 보호 middleware가 Refresh로 Access만 갱신한다.
HTTP 개발 예외 설정·쿠키 이름·캐시·오류·브라우저 미검증 범위는 [JWT 웹 연결](docs/specs/auth/jwt-web/plan.md)을 참조한다.
UI를 같은 origin에서 제공하는 구성이 전제이며 별도 Vite 화면의 로그인 연동은 아직 없다.
VSA 배치·entity 기반 저장소 API·오류 계약은 [로컬 사용자 추가 설계](docs/specs/auth/create-local-user/plan.md)를 참조한다.

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
TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab-app --locked -- --include-ignored
```

위 URL은 공개된 로컬 개발용 기본값이다. 포트를 변경했다면 테스트 URL에도 반영한다.

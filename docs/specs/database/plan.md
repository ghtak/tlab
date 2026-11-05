---
id: PLN-DATABASE
status: accepted
---

# SQLx 연결·트랜잭션 설계

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| realizes | [SPEC-DATABASE](./spec.md) | SQLx·PostgreSQL 최초 범위, REQ-DATABASE-001–005 |
| realizes | [REQ-ERROR](../error/requirements.md#req-error-004) | DB 오류 분류·변환·원인 보존 |

## 설계 시작 시점의 상태·범위·승인

- 현재 `tlab`은 설정·공통 에러만 제공한다. SQLx 모듈·의존성·호출자는 없다. 기존 `config::load`, `Error`, `Result`를 재사용한다. (확인)
- 요구사항 승인 근거는 SQLx·PostgreSQL 제안에 대한 사용자 “설계 진행해” 요청이다. 사용자가 알린 가동 환경을 조회하여 PostgreSQL 18.6의 healthy 상태와 공개 포트 `127.0.0.1:5432`를 확인했다. (확인)
- 설계 제안 후 사용자가 “구현및 코드리뷰까지 진행해”라고 요청하여 본 설계와 구현·검증·리뷰를 함께 승인했다. Oracle·Valkey·ORM·마이그레이션·자동 재시도는 이번 범위에 포함하지 않는다. (확인)
- [구현 가이드](../../architecture/guide-impl.md)·[테스트·검증 가이드](../../architecture/guide-test.md)를 적용한다. 백서의 얇은 문맥을 유지하고 수명·savepoint·풀 관리를 SQLx에 위임한다. (추론)

## 구성과 책임

```mermaid
flowchart LR
    Settings[호출자 AppConfig] --> Loader[기존 config::load]
    Loader --> Config[sqlxdb::Config]
    Config --> DB[Database: SQLx Pool 소유]
    DB --> Conn[Conn: 확보한 연결 소유]
    DB --> Tx[Tx: SQLx Transaction 소유]
    Conn --> Tx
    Tx --> Nested[Tx: 빌린 하위 savepoint]
    Conn --> Ctx[Context: 연결 가변 대여]
    Tx --> Ctx
    Nested --> Ctx
    Ctx --> Repo[호출자 저장소 함수: SQLx 쿼리]
    Repo --> Error[기존 tlab::Result와 새 Error::DbError]
```

`Context`가 소유권·트랜잭션 상태를 따로 관리하지 않고 `&mut DB::Connection`만 짧게 빌린다. SQL·행 모델·업무 오류 해석은 소비 애플리케이션의 책임이다. `Context` 자체에 `Executor`·`Acquire`를 구현하거나 드라이버 공통 trait을 추가하지 않는다. (추론: 설계 제안)

| 파일 | 변경과 책임 |
| --- | --- |
| `tlab/src/sqlxdb.rs` | `Config`, `PoolStatus`, `Database<DB>`, `Conn<DB>`, `Tx<'c, DB>`, `Context<'c, DB>` 추가 |
| `tlab/src/lib.rs` | `pub mod sqlxdb;` 추가, DB 타입의 루트 재노출은 하지 않음 |
| `tlab/src/error.rs` | 원본 SQLx 오류를 보관하는 `DbError`와 `From` 추가, 기존 종류·변환 유지 |
| `tlab/Cargo.toml`, `Cargo.lock` | SQLx와 테스트용 Tokio 추가·잠금 갱신 |
| `tlab/tests/sqlxdb.rs` | 외부 호출자 관점의 설정·일반 쿼리·트랜잭션·오류·풀 상태 검증 |
| `tlab/tests/error.rs`, 에러 문서 테스트 | 기존 오류 회귀, DB 변환과 `non_exhaustive` 예제 갱신 |
| `docs/architecture/index.md` | 구현 후 DB 모듈과 본 설계 링크 반영 |

위 변경 목록은 구현 완료했다. 실행 결과·리뷰는 하단에 기록한다. (확인)

## 의존성·지원 정책

```toml
# tlab/Cargo.toml에 추가할 제안
[dependencies]
sqlx = { version = "0.9", default-features = false, features = ["postgres", "runtime-tokio", "tls-rustls-ring-native-roots"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
serde_json = "1" # PoolStatus 직렬화 검증에만 사용
```

- SQLx 0.9.0의 공식 문서와 로컬 소스에서 API·feature·Rust 최소 버전 1.94를 확인했다. 현재 `rustc`는 1.97.1이다. 해석된 버전·전체 컴파일·보안 검사는 구현 단계에서 확정한다. [공식 문서](https://docs.rs/sqlx/0.9.0/sqlx/) (확인)
- 최초 지원·검증 대상은 PostgreSQL과 Tokio다. 타입은 백서처럼 제네릭으로 유지하되 MySQL·SQLite까지 지원한다고 표시하지 않는다. SQLx와 동일한 0.9 계열을 소비자가 사용해야 공개 타입이 호환된다. (추론)
- 새 `tlab` feature 계층은 만들지 않고 DB 기능을 기본 제공한다. 기존 크레이트의 의존성 선언 방식에 맞춘 최소안이며 빌드 의존성은 증가한다. 드라이버 선택을 옵션화할 실제 요구가 생기면 별도로 다룬다. (추론)
- TLS는 Rustls·시스템 인증서를 선택한다. 서버의 TLS 요구·인증서 검증은 SQLx 연결 옵션에 위임하며 `tlab`이 강제로 비활성화하지 않는다. 로컬 비TLS 접속 성공으로 TLS 동작까지 검증했다고 보고하지 않는다. (추론)
- SQLx의 query macro·derive·migrate·any 기본 feature는 활성화하지 않는다. 테스트·예제는 `sqlx::query`·`query_scalar`를 사용하여 빌드 시 DB 접속을 요구하지 않는다. (추론)

## 공개 인터페이스

아래는 공개 계약이다. 필드는 별도 표기한 설정·상태 외에는 비공개로 두며, 실제 메서드 본문은 구현 단계에 작성한다. (추론)

```rust
// tlab::sqlxdb
#[derive(serde::Deserialize)]
pub struct Config {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PoolStatus {
    pub max_connections: u32,
    pub size: u32,
    pub idle: usize,
}

pub struct Database<DB: sqlx::Database> { /* sqlx::Pool<DB> */ }
pub struct Conn<DB: sqlx::Database> { /* sqlx::pool::PoolConnection<DB> */ }
pub struct Tx<'c, DB: sqlx::Database> { /* sqlx::Transaction<'c, DB> */ }
pub struct Context<'c, DB: sqlx::Database> { /* &'c mut DB::Connection */ }
```

| 타입·작업 | 시그니처 |
| --- | --- |
| 풀 생성 | `Database::<DB>::new(config: &Config) -> impl Future<Output = tlab::Result<Self>>` |
| 풀 상태 | `Database::pool_status(&self) -> PoolStatus` |
| 연결 획득 | `Database::conn(&self) -> impl Future<Output = tlab::Result<Conn<DB>>>` |
| 풀의 루트 트랜잭션 | `Database::tx(&self) -> impl Future<Output = tlab::Result<Tx<'static, DB>>>` |
| 연결의 트랜잭션 | `Conn::begin(&mut self) -> impl Future<Output = tlab::Result<Tx<'_, DB>>>` |
| 연결 문맥 | `Conn::context(&mut self) -> Context<'_, DB>` |
| 하위 트랜잭션 | `Tx::begin(&mut self) -> impl Future<Output = tlab::Result<Tx<'_, DB>>>` |
| 트랜잭션 문맥 | `Tx::context(&mut self) -> Context<'_, DB>` |
| 완료·취소 | `Tx::commit(self)` / `Tx::rollback(self) -> impl Future<Output = tlab::Result<()>>` |
| 쿼리 연결 | `Context::backend(&mut self) -> &mut DB::Connection` |

비동기 메서드는 `pub async fn`으로 구현한다. `Database`, `Conn`, `Tx`, `Context`는 `Clone`을 제공하지 않는다. `Config`에는 URL 자격 증명이 있을 수 있으므로 자동 `Debug`를 제공하지 않는다. 원본 SQLx 연결을 노출하므로 호출자가 직접 transaction SQL을 실행해 래퍼의 경계를 우회하는 사용은 지원하지 않는다. (추론)

### 생성·풀·상태

- `Config` 두 필드는 필수이며 숨은 기본값을 추가하지 않는다. 기존 `config::load`에서 중첩 설정으로 읽을 수 있다. `url`이 비었거나 공백뿐인 경우, `max_connections == 0`은 접속 전에 `Error::InvalidInput`으로 거부한다. URL을 임의로 trim·변경하지 않는다. 그 외 URL 파싱·인증·연결 실패는 원본 SQLx 오류를 보존한다. (추론)
- `PoolOptions::<DB>::new().max_connections(...).connect(...)`로 최소 한 연결의 성공을 확인한 후 반환한다. 최대 연결 수 외의 옵션·획득 대기·연결 검사·idle 수명은 SQLx 기본값을 따른다. 0.9.0 소스의 획득 제한 기본값은 30초이나 `tlab`의 영구 고정값으로 보장하지 않는다. 자체 재시도는 추가하지 않는다. (확인: 소스 / 추론: 위임 정책)
- `pool_status`는 SQLx의 최대 설정·`size()`·`num_idle()`을 읽고 `idle = num_idle.min(size as usize)`로 제한한다. 동시 변경 중 관측값이며 원자적 스냅샷은 아니다. 조회 중에는 연결을 획득하지 않는다. (추론)
- 연결 반환·풀 마지막 핸들 해제는 SQLx의 수명 관리에 맡긴다. 명시적 종료·강제 취소 API는 이번에 추가하지 않는다. (추론)

### 트랜잭션·소유권·실패

- `Database::tx`는 SQLx 풀의 루트 트랜잭션을 소유하므로 반환 수명을 `'static`으로 둔다. 백서의 `Tx<'_, DB>`보다 불필요한 풀 참조 제약을 줄이고 SQLx API와 맞춘다. `'static`은 실제 연결의 영구 유지를 의미하지 않는다. (확인: SQLx `Pool::begin` 소스 / 추론: 선택)
- `Conn::begin`은 빌린 실제 연결에서 시작하고, `Tx::begin`은 `sqlx::Acquire::begin(&mut inner)`를 통해 활성 트랜잭션의 savepoint를 생성한다. `Tx`를 먼저 연결로 역참조해 별도 루트 트랜잭션을 만들지 않는다. (추론)
- 자식 트랜잭션·문맥의 가변 대여가 살아 있는 동안 부모에서 경쟁 쿼리·완료 작업을 할 수 없다. 문맥은 복제되지 않으며 `backend()`는 호출마다 짧은 재대여를 반환한다. `commit(self)`·`rollback(self)` 후 핸들을 재사용할 수 없다. (추론)
- 하위 commit은 savepoint 해제이고 최종 확정은 루트 commit이다. 하위 rollback 이후 외부 트랜잭션은 계속 쓸 수 있으며 루트 rollback은 하위 commit을 포함한 전체 변경을 취소한다. DB SQL을 직접 조합하지 않고 SQLx 트랜잭션 관리자에 맡긴다. [공식 트랜잭션 문서](https://docs.rs/sqlx/0.9.0/sqlx/struct.Transaction.html) 및 로컬 소스를 확인했다. (확인: 드라이버 의미 / 추론: 위임)
- 미완료 핸들 해제는 SQLx `Transaction::Drop`의 rollback 시작에 위임한다. drop 자체가 비동기 rollback 완료나 오류 보고를 보장하지 않으므로 확실한 결과가 필요하면 명시적으로 `rollback().await`한다. 자체 `Drop`·spawn·rollback 재시도를 추가하지 않는다. (확인: SQLx 소스 / 추론: 계약)
- PostgreSQL 문장 오류로 트랜잭션이 실패 상태가 되면 호출자가 해당 트랜잭션 또는 savepoint를 rollback한다. `tlab`은 자동 복구·재실행하지 않는다. commit 실패·future 취소는 서버 반영 여부가 불명확할 수 있으므로 무조건 미반영으로 보고하지 않으며, 재사용 정리는 SQLx에 맡긴다. 취소 시 원래 연결 identity의 유지까지 보장하지 않는다. (추론)

### 공통 오류와 호출 예

```rust
// 기존 Error에 추가할 종류
#[error("database error: {0}")]
DbError(#[from] sqlx::Error),
```

`From<sqlx::Error>`를 유일한 기본 변환으로 사용하고 원본과 `source()`를 보존한다. 드라이버별 `map_error` 함수는 추가하지 않는다. `RowNotFound`·제약 위반·연결 실패 등 SQLx 오류는 모두 이 종류에 유지하고 업무별 의미는 호출자가 판단한다. 기존 `Internal(anyhow::Error)`의 변환 의미는 유지한다. (추론)

```rust
type AppDb = tlab::sqlxdb::Database<sqlx::Postgres>;
type AppDbCtx<'a> = tlab::sqlxdb::Context<'a, sqlx::Postgres>;

async fn read_value(ctx: &mut AppDbCtx<'_>) -> tlab::Result<i32> {
    Ok(sqlx::query_scalar("SELECT 42::integer")
        .fetch_one(ctx.backend()).await?)
}

async fn use_database(db: &AppDb) -> tlab::Result<()> {
    let mut conn = db.conn().await?;
    read_value(&mut conn.context()).await?;
    let mut tx = conn.begin().await?;
    read_value(&mut tx.context()).await?;
    tx.rollback().await
}
```

위 사용 패턴은 공개 모듈 문서 예제와 통합 테스트에서 컴파일·실행으로 확인했다. (확인)

## 검증 설계

기존 [로컬 DB 환경](../../verification/feature/database-environment.md)과 [접속 도구](../../verification/tools/database/README.md)를 재사용한다. 컨테이너를 재생성하거나 공유 데이터를 초기화하지 않는다. 기본 URL은 저장소에 공개된 로컬 개발 계정의 `postgres://tlab:tlab_dev@127.0.0.1:5432/tlab`이며 통합 테스트의 `TLAB_TEST_DATABASE_URL`로 명시한다. 테스트가 환경변수를 수정하거나 `.env`를 자동 로딩하지 않는다. (추론)

DB 통합 테스트는 `#[ignore = "requires local PostgreSQL"]`로 구분하고 전용 명령에서 명시 실행한다. 전용 실행에서 URL 누락·접속 실패를 성공 처리하거나 조용히 건너뛰지 않는다. 비DB 테스트·문서 컴파일은 일반 `cargo test`에 포함한다. (추론)

| 요구사항 | 실행 경로와 기대 결과 |
| --- | --- |
| [001](./spec.md#req-database-001) | 기존 로더로 Config 구성, 정상 DB 연결·`SELECT` 성공. 빈 URL·0개 연결은 InvalidInput, 잘못된 URL은 DbError, 호스트 TCP 연결 성공 확인 |
| [002](./spec.md#req-database-002) | 동일 저장소 함수를 `Conn`·루트 `Tx`·하위 `Tx` 문맥에서 호출. 문맥의 수명 초과·활성 자식과 부모 완료 충돌·완료한 핸들 재사용은 작은 compile-fail 문서 예제로 검증 |
| [003](./spec.md#req-database-003) | commit·rollback의 실제 값, 하위 rollback 후 외부 commit, 하위 commit 후 루트 rollback, 하위·루트 drop 후 후속 쿼리로 미확정 확인. 문장 오류 후 savepoint rollback으로 외부 작업 지속 확인 |
| [004](./spec.md#req-database-004) | 명시 변환·`?`·실제 SQL 실패의 DbError 분류·원본·SQL 상태 확인. 기존 설정·I/O·anyhow 변환 회귀 유지 |
| [005](./spec.md#req-database-005) | 전용 풀에서 최대·size·idle 및 직렬화 필드 확인. 최대 연결을 확보한 상태에서도 동기 조회가 가능함을 확인. 연결 반환은 SQLx 비동기 처리이므로 제한 시간 내 안정화로 검증 |

연결에서 시작하는 트랜잭션은 세션 임시 테이블을 사용해 다른 테스트와 격리한다. `Database::tx`의 풀 획득 경로는 고유 이름의 테스트 전용 테이블로 확정·취소를 다른 연결에서 확인하고 생성한 테이블만 정리한다. 기존 테이블을 삭제하지 않으며 실패 경로의 정리도 처리한다. 타이밍·idle 개수는 임의 sleep이나 동시 실행 중 고정값 대신 전용 풀과 제한 시간 내 상태 확인으로 검증한다. (추론)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
# 별도 DB 실행: 아래 변수는 이 명령에만 적용
TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --locked --test sqlxdb -- --ignored
cargo audit
```

구현 시 새 의존성 해석·잠금 갱신 후 위 명령을 실행한다. Rust 통합 테스트에서 호스트 TCP·SQLx·`tlab` 공개 경로를 검증하며 컨테이너 내부 검사 성공을 대체 근거로 사용하지 않는다. TLS·다른 DB·부하 성능·commit 중 통신 단절의 장애 주입은 이번 실행 검증 범위에 포함하지 않는다. 취소·통신 실패에 대한 확정 결과를 추가로 보장하려면 별도 요구사항·검증이 필요하다. (추론)

## 결정사항·남은 제약

| 논점 | 선택과 이유 | 대안·영향 | 상태 |
| --- | --- | --- | --- |
| 실행 경계 | 가변 대여 Context, 백서의 저장소 재사용 목적 유지 | SQLx 연결 직접 전달은 가능하지만 명시한 문맥 경계가 사라짐 | 설계 제안 |
| 지원 DB | 제네릭 타입·PostgreSQL 최초 지원 | 모든 드라이버의 일괄 검증·지원은 범위 확대 | 요구사항 승인, 세부 API 제안 |
| 루트 수명 | 풀에서 얻은 Tx는 `'static`, 연결·하위 Tx는 대여 수명 | 풀 참조에 묶으면 실제 SQLx 소유권보다 제한적 | 설계 제안 |
| 오류 | DbError(sqlx::Error)·From | 백엔드별 매핑은 지금 동일 변환을 중복 | 설계 제안 |
| 의존성 | SQLx 0.9·Tokio·Rustls, 필요한 feature만 사용 | 0.8 소비자는 타입 호환을 위해 업그레이드 필요 | 설계 제안 |
| 검증 환경 | 기존 PostgreSQL·just 재사용 | 별도 DB 도구·테스트 환경 신설 불필요 | 환경 확인, Rust 검증 미실행 |

위 설계 제안은 후속 구현·리뷰 요청으로 채택되었다. TLS 실연동과 외부 소비자 구성은 미확인 범위다. (확인)

## 구현·검증 결과

- 구현: [sqlxdb.rs](../../../tlab/src/sqlxdb.rs)에 설정·풀 상태·제네릭 풀·연결·트랜잭션·문맥을 추가했다. 가변 대여로 수명을 제한하고 SQLx의 풀·savepoint·미완료 rollback을 재사용한다. [error.rs](../../../tlab/src/error.rs)에 `DbError(sqlx::Error)`와 From을 추가했다. 기존 종류·변환·설정 로더·예제는 유지한다. (확인)
- 의존성: SQLx/core/postgres 0.9.0, Tokio 1.54.1, 테스트용 serde_json 1.0.154로 잠갔다. 기존 잠금 파일에 있던 패키지의 버전은 바꾸지 않았다. `serde_json` 직접 개발 의존성은 PoolStatus의 실제 직렬화 필드 검증을 위해 추가했다. (확인)
- 실제 검증 환경: 2026-11-04 05:53 UTC 기준, `aarch64-apple-darwin`, Rust·Cargo 1.97.1, Clippy 0.1.97, rustfmt 1.9.0-stable, cargo-audit 0.22.2, PostgreSQL 18.6. 기본 feature·호스트 target에서 실행했다. (확인)

| 명령 | 결과·검증 범위 |
| --- | --- |
| `cargo fmt --all -- --check` | 통과 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 통과, 경고 없음 |
| `cargo audit` | 통과, advisory 1,403건으로 잠금 파일 의존성 199개 검사, 차단 항목 없음 |
| `cargo test --workspace --locked` | 통과: 단위 1개, 설정 통합 7개, 에러 통합 6개, DB 비접속 테스트 2개, 문서 테스트 7개 |
| `TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --locked --test sqlxdb -- --ignored` | 통과: 실제 DB 통합 4개, 호스트 TCP와 tlab 공개 API 확인 |

- 일반 테스트의 ignored는 실제 DB 테스트 4개와 기존 설정 테스트용 자식 프로세스 helper 1개다. DB 4개는 별도 명령으로 모두 실행했고 설정 helper는 기존 상위 테스트가 실행했다. (확인)
- [DB 테스트](../../../tlab/tests/sqlxdb.rs)는 설정 조합·입력 검증, 동일 저장소 함수의 연결·루트·savepoint 사용, 하위 rollback 후 외부 commit, 하위 commit 후 전체 rollback, 하위·루트 Drop, SQL 오류 후 savepoint rollback·외부 작업 지속, 풀 루트 commit·rollback·Drop, SQL 상태 22012·원본 보존, 풀 고갈 중 상태 조회·직렬화·연결 반환을 확인했다. 고유 스키마는 값을 조회한 뒤 정리하고 마지막으로 기대값과 비교하도록 구성했으며 기존 데이터는 초기화하지 않았다. (확인)
- 문서 테스트는 문맥 수명 초과·활성 하위 트랜잭션과 부모 완료의 충돌·완료 핸들 재사용의 컴파일 거부를 확인한다. (확인)
- 최초 offline 의존성 해석은 캐시의 chacha20 버전 문제로 실패했으나 온라인 해석에서 rand 0.10.3·chacha20 0.10.2를 선택하여 정상 빌드했다. 최초 테스트 컴파일은 SQLx 0.9의 `SqlSafeStr` 제약을 확인해 고유 스키마·테이블의 동적 SQL을 `QueryBuilder`로 변경한 뒤 통과했다. 숫자로만 구성한 식별자를 사용하고 삽입 값은 바인딩한다. (확인)
- 최초 DB 실행은 샌드박스 TCP 접속 제한(`Operation not permitted`)으로 4개 실패했다. 권한 승인 후 같은 명령을 다시 실행해 모두 통과했다. 테스트 실패를 건너뛰는 우회는 추가하지 않았다. (확인)
- audit DB 기준은 `965bf9ad083b58e6340d65829d85217b18be9290`이다. advisory·경고 예외를 적용하지 않았다. Miri는 직접 작성한 unsafe·메모리 조작·관련 동시성 구현 변경이 없어 비대상이다. Geiger는 별도 unsafe 현황 자료가 필요하지 않아 실행하지 않았으며 의존성 내부의 unsafe 부재를 의미하지 않는다. (확인: 실행 / 추론: 조건부 적용)

## 코드 리뷰

### 대상·선행 검사

- 비교 기준: `31055ffc7e384d7f38ec815fd34356e342338e2c` 대비 미커밋 DB 변경. `sqlxdb.rs`와 테스트, 공통 에러·공개 모듈·Cargo 변경 및 관련 README·아키텍처·DB 명세를 검토했다. 백서는 분석 원문으로 유지했다. (확인)
- 코드·Cargo·테스트 7개 파일의 스냅샷 SHA-256은 `7dbe84c248d794742941248387b6f69f6e0b55cb687000c7714736ec36842b4a`다. 경로를 사전순 정렬한 뒤 `경로 UTF-8 + NUL + 내용 + NUL`을 이어 해시했으며 대상은 Cargo.lock, tlab/Cargo.toml, src의 lib.rs·error.rs·sqlxdb.rs, tests의 error.rs·sqlxdb.rs다. (확인)
- 형식 → Clippy → audit의 필수 선행 검사 통과 후 LLM 리뷰를 수행했다. 위 실제 테스트 결과를 동일 코드의 근거로 재사용했다. 작성자가 수행한 리뷰이며 별도 독립 리뷰어의 평가는 아니다. (확인)

### 발견 사항과 조치

- 최종 소스에서 수정이 필요한 치명·주요·경미 결함을 발견하지 못했다. 공개 수명·독점 대여·savepoint 분기·오류 원인·입력 검증·풀 상태와 관련 테스트를 설계 및 요구사항 001–005에 대조했다. (추론: 검토 결과)
- 실행 중 발견한 동적 SQL 컴파일 오류는 QueryBuilder로 수정 후 전체 검사·DB 테스트를 통과했다. 테스트용 스키마는 SQL 오류 시에도 정리를 시도하고 값 비교 assertion은 정리 이후에 수행한다. (확인)
- 아키텍처의 DB 미구현 설명과 공통 에러 목록을 현재 구현에 맞게 갱신하고 README에 실제 DB 테스트 명령을 추가했다. (확인)

### 품질 점수·판정

가이드의 초기 제안 기준을 이번 변경 범위에 적용했다. 도구 통과만으로 평가하지 않고 공개 계약·코드·실행 근거를 함께 대조했다. (확인: 적용 기준)

| 항목 | 점수 | 근거 |
| --- | --- | --- |
| 기능 | 4/4 | 요구사항 001–005의 정상·실패·수명·상태를 공개 API·실제 PostgreSQL로 검증 |
| 가독성과 구조 | 4/4 | 단일 모듈에서 각 타입의 소유·대여 책임 구분, SQLx에 기존 기능 위임, 드라이버별 중복 매핑 없음 |
| 안정성 | 4/4 | 가변 대여·소비형 완료, savepoint 오류 후 복구·Drop·풀 반환 검증, 취소·commit 불확실성 명시 |
| 보안 | 4/4 | 설정 Debug 미제공, 자체 로그 없음, 쿼리 값 바인딩·숫자 기반 테스트 식별자, 원본 오류 보존·audit 통과 |
| 성능 | 4/4 | 문맥은 연결 참조만 보관, 추가 연결 획득·복제·자체 재시도 없음, 상태 조회는 동기 관측 |
| 테스트 용이성 | 4/4 | 공개 API 경로, 명시적 DB URL, 세션 임시 테이블·고유 스키마 격리, 상태 안정화 제한 시간·컴파일 거부 검증 |

- 판정: **통과**, 품질 **100/100**, 리스크 **R2**. 이는 이번 변경 범위의 리뷰 판정이며 배포 승인이나 미검증 환경의 보장이 아니다. (추론)
- 아키텍처 변경: 있음. 소비자 → tlab SQLx 수명·오류 래퍼 → SQLx → PostgreSQL의 접근 경계와 공개 타입·의존성을 추가했다. 기존 설정·에러 모듈과 단일 크레이트 구조를 유지하고 SQL·행 모델은 소비자에게 남긴다. 공개 타입을 쓰는 소비자는 SQLx 0.9 계열을 사용해야 한다. (확인: 전후 구조 / 추론: 분류)
- 복구 가능성: 저장소의 이번 변경은 양방향이다. 라이브러리를 추가했을 뿐 기존 스키마·데이터를 변환하지 않았고 테스트에서 만든 스키마는 정리했다. 소스·Cargo·잠금·테스트·문서를 함께 복원하면 기존 API 동작으로 돌아갈 수 있다. 외부 소비자가 새 API를 채택했다면 소비자 코드·의존 버전도 함께 복원해야 한다. (확인: 부수 효과 / 추론: 복구)
- 트랜잭션 래퍼를 복원해도 소비 애플리케이션이 이미 commit한 업무 데이터는 취소되지 않는다. 업무 데이터 복구는 소비자의 책임이며 이번 작업에는 운영 데이터 변경·배포·실제 롤백 실행이 없다. (확인: 수행 범위 / 추론: 경계)
- 미확인 범위: TLS 실접속, 다른 OS·DB·Rust 버전, 외부 소비자 통합, 성능 부하 측정, commit 중 통신 단절·취소의 장애 주입, 실제 롤백. 풀의 세부 자원 정리·통신 취소는 SQLx에 위임하며 그보다 강한 보장을 추가하지 않았다. (확인)

## 구현·변경 기록

### 최초 설계

- 계기: 사용자가 요구사항 정리 후 설계 진행을 요청하고 `just db-up` 환경이 가동 중임을 알렸다. (확인)
- 반영: 공개 API·가변 대여·풀 소유 트랜잭션의 수명·savepoint·Drop 위임·공통 오류·의존성·요구사항별 검증 방법을 정리했다. (확인)
- 환경 확인: `just db-status`는 Docker 소켓 샌드박스 제한으로 실패한 뒤 권한 승인 재실행으로 세 DB의 healthy 상태를 확인했다. `just db-check-postgres`는 승인받아 실행하여 `PostgreSQL read/write OK`를 확인했으며 임시 테이블 생성·조회 뒤 rollback했다. 호스트 Rust 연결은 미실행이다. (확인)
- 소스·의존성 변경, Rust 테스트·audit 실행, 컨테이너 재시작·초기화는 수행하지 않았다. 설계 승인 후 구현·검증 단계로 진행한다. (확인)

### 1차 구현·검증·리뷰

- 계기·승인: 사용자 “구현및 코드리뷰까지 진행해” 요청으로 설계를 채택하고 구현·검증·리뷰와 필요한 수정을 함께 수행했다. (확인)
- 결과: SQLx 공개 모듈·공통 DB 오류·설정 조합·실제 DB 테스트·문서 테스트를 구현하고 필수 검사와 정식 리뷰를 완료했다. 상세 근거는 위 구현·검증 결과 및 코드 리뷰에 있다. (확인)
- 설계 대비 차이: PoolStatus 직렬화 검증용 serde_json 개발 의존성을 명시했다. SQLx 0.9의 동적 SQL 제약을 테스트에서 QueryBuilder로 처리했으며 공개 API·트랜잭션·오류 계약은 승인된 설계와 같다. (확인)
- 최종 자체 점검: 요구사항·호출자·실패·부수 효과·복구 경계를 최종 변경과 대조했고 추가 수정 사항은 남아 있지 않다. 소스·데이터·컨테이너의 관련 없는 변경을 섞지 않았다. (추론: 점검 결과)
- 커밋·PR·게시·배포는 수행하지 않았다. (확인)

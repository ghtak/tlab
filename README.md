# tlab

공통 기반 기능을 제공하는 Rust 라이브러리.

SQLx 기반 풀·연결·트랜잭션은 `tlab::sqlxdb`에서 제공한다. 최초 지원 대상은
Tokio·PostgreSQL이며 쿼리는 SQLx 0.9 API를 사용한다.
공개 계약과 검증 결과는 [DB 설계](docs/specs/database/plan.md)를 참조한다.

로컬 테스트용 PostgreSQL·Valkey·Oracle Free 환경은 [DB 검증 도구](docs/verification/tools/database/README.md)를 참조한다.
Docker·Compose·just를 준비한 뒤 프로젝트 루트에서 실행한다.

```sh
just db-up
just db-check
just db-down
```

DB 통합 테스트는 환경을 가동한 상태에서 명시적으로 실행한다. URL이 없거나
접속에 실패하면 테스트가 실패한다. 일반 `cargo test --workspace --locked`에서는
외부 DB를 사용하는 4개 테스트를 제외한다.

```sh
TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --locked --test sqlxdb -- --ignored
```

위 URL은 공개된 로컬 개발용 기본값이다. 포트를 변경했다면 테스트 URL에도 반영한다.

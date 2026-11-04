# tlab

공통 기반 기능을 제공하는 Rust 라이브러리.

로컬 테스트용 PostgreSQL·Valkey·Oracle Free 환경은 [DB 검증 도구](docs/verification/tools/database/README.md)를 참조한다.
Docker·Compose·just를 준비한 뒤 프로젝트 루트에서 실행한다.

```sh
just db-up
just db-check
just db-down
```

---
id: PLN-AUTH-JWT-REFRESH-STORAGE
status: accepted
---

# Refresh 저장 구현과 검증

## 범위와 책임

[JWT 정책](../jwt.md), [구현 순서](../jwt-todo.md), [토큰 기반](../jwt-token/plan.md)과 기존 0002 SQL을 기준으로 auth entity와 repository, 계정 행 잠금, 실제 PostgreSQL 테스트를 구현했다. 로그인·갱신·종료 usecase와 HTTP는 포함하지 않는다. (확인)

`NewRefreshToken`은 기존 `RefreshHash`와 호출자가 제공한 UUID·시각을 받는다. 원문은 repository 입력에 없고 entity에도 Debug/Serialize가 없다. 저장 행은 해시 바이트를 반환한다. `find_by_hash`는 폐기·만료를 걸러내지 않으며 `lock_by_hash`는 행을 다시 읽어 잠근다. `revoke_session`은 계정과 로그인 ID를 함께 조건으로 사용하고 `revoke_account`는 해당 계정의 미폐기 행을 갱신한다. 두 함수는 새로 폐기한 행 수를 반환하며 기존 폐기 시각·사유는 변경하지 않는다. 만료된 미폐기 행도 폐기 대상이다. (확인)

트랜잭션과 잠금 정책은 호출 시나리오가 소유한다. 로그인과 계정 상태 변경·전체 폐기의 조합은 같은 트랜잭션에서 계정 행을 먼저 잠근다. 갱신은 일반 연결에서 Refresh와 계정을 잠금 없이 조회하며 명시적 트랜잭션을 시작하지 않는다. 일반 로그아웃은 선행 계정·Refresh 조회 잠금 없이 해시 조회 후 미폐기 행을 조건부 UPDATE하고 commit한다. UPDATE 자체의 행 잠금은 발생한다. `lock_by_hash`를 사용하는 경로만 같은 트랜잭션에서 계정 잠금 이후 Refresh를 잠그고 재조회한다. repository는 이 정책을 자동 강제하거나 로그인 허용·만료를 판단하지 않는다. (확인)

로그인 발급 호출자는 계정 잠금 획득 후 현재 시각을 구하고 최초 로그인부터 168시간의 세션 한도와 동일한 Refresh 만료를 지정한다. 삽입의 `created_at`도 명시적으로 받는다. PostgreSQL `CURRENT_TIMESTAMP`는 트랜잭션 시작 시각이므로 잠금 대기 후 구한 로그인 시각보다 앞설 수 있기 때문이다. 더 짧은 토큰 만료와 세션 만료는 별도 필드로 유지한다. (확인·추론)

갱신·로그아웃 또는 계정 상태 변경이 겹치면 진행 중인 갱신은 이전 상태로 Access를 발급할 수 있다. 완료된 폐기를 이후에 조회한 갱신은 거부한다. 동일 Refresh의 반복·동시 갱신과 재시도는 저장 행을 변경하지 않으며 `expires_at`과 `session_expires_at` 어느 쪽도 연장하지 않는다. 현재 두 만료는 최초 로그인부터 168시간으로 고정된다. 성공한 갱신은 발급 시각부터 600초 유효한 새 Access만 반환한다. 상세 경합과 조회 시점의 한계는 [갱신·종료 계약](../jwt-refresh-lifecycle/plan.md#계약과-책임)을 따른다. (확인)

## SQL 변경과 호환성

기존 SQL은 UUID 기본값을 v4로 생성하지만 명시적으로 전달한 nil이나 다른 버전도 허용했다. UUID 바이트의 버전과 RFC 4122 variant를 검사하는 CHECK를 추가했다. 기존 유효 v4 값과 신규 기본값에는 영향이 없고 잘못된 값은 거부한다. `CREATE TABLE IF NOT EXISTS`는 기존 테이블의 제약을 바꾸지 않으므로 이미 적용된 DB에는 자동 반영되지 않는다. 기존 데이터 점검과 별도 마이그레이션이 필요하며 이번 작업에서는 기존 DB 스키마나 사용자 데이터를 변경하지 않았다. (확인)

## 검증 결과

- 로컬 개발 PostgreSQL의 무작위 이름 임시 스키마에 0001·0002를 적용하고 테스트 후 스키마를 제거했다. `TLAB_TEST_DATABASE_URL=… cargo test -p tlab-app auth::repository::refresh_token --locked --offline -- --ignored`: 1 passed. 운영 DB는 사용하지 않았다. (확인)
- 실제 생성 토큰의 해시만 저장·반복 조회, 두 만료 필드의 독립성, 해시 중복·동일 세션의 두 번째 미폐기 행·nil UUID 거부를 확인했다. 기기별 폐기 격리, 계정 전체 폐기와 다른 계정 보존, 중복 폐기 시 최초 시각·사유 보존, 삽입과 폐기 rollback을 확인했다. (확인)
- 두 트랜잭션에서 계정 잠금 대기, 선행 트랜잭션의 계정→Refresh 잠금 및 전체 폐기 commit, 대기자의 계정→Refresh 잠금 후 폐기 상태 재조회가 이루어짐을 확인했다. 모든 향후 usecase가 이 순서를 지키는지는 아직 검증할 수 없다. (확인)
- `cargo test --workspace --locked --offline`: 통과, DB 등 ignored 테스트는 별도 실행한 위 테스트를 제외하고 자동 실행하지 않았다. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `git diff --check`: 통과. 전체 회귀 및 clippy는 최종 테스트 보강 전 실행했고, 보강 후 DB 테스트와 포맷 검사를 다시 실행했다. (확인)

## 미완료와 후속 연결

요청한 저장 범위의 미완료 항목은 없다. 실제 로그인·7일 경계의 허용/거부·반복 갱신과 상태 변경의 직렬화는 후속 usecase에서 구현하고 검증해야 한다. 기존 DB에 새 CHECK를 적용하는 마이그레이션도 별도 운영 작업이다. 쿠키·CSRF·JWKS 및 기존 토큰 코드의 리뷰 개선은 범위 밖이다. (확인)

## 코드 리뷰

### 발견 사항

- **[주요: 검증 누락] Refresh 행 잠금의 경합을 직접 검증하지 않는다.** `tlab-app/src/auth/repository/refresh_token/tests.rs:254`의 경합은 두 트랜잭션이 계정 행을 잠그는지 확인하며, 두 번째 트랜잭션은 첫 번째가 commit한 후에야 `lock_by_hash`를 호출한다. 따라서 `lock_by_hash`에서 `FOR UPDATE`가 제거되어도 이 경합 테스트는 통과할 수 있다. 구현 SQL에는 `FOR UPDATE`가 있지만, 요청한 실제 PostgreSQL 잠금 계약의 독립적인 회귀 검증은 부족하다. 별도 두 트랜잭션에서 같은 Refresh 행의 잠금 획득이 선행 트랜잭션 종료까지 대기하고 이후 최신 행을 반환하는지 확인해야 한다. 계정 잠금 경합 테스트도 유지한다. 이는 구현된 잠금이 잘못되었다는 재현 결과가 아니라 검증 누락이다. (확인·추론)

### 대상·검사·판정

- 대상: `HEAD 675d15852c26da7ca2a15478cce96a3beb1c7b50` 대비 이 문서에 기록된 미커밋 Refresh 저장 변경. `refresh_token.rs` SHA-256 `b219bd3afaf8788e92f7c19b5c27c79fae7ee4e2890fdbb1dc65c67b9ff3825a`, 테스트 `610c32945a14e795ae221bc1134cde7ab8f1b1120703c38a184cfd954cd66b2d`, entity `80591fc14ebc3c8b5d0f8b15a329de83b4f4c192234da96ae5ddd5907bea8826`, migration `0e803df7b2de2d1ad997df502a4a88c8a8ed8460b7c73e04f8c36236b7fab281`, Cargo.lock `ddcb53212b3e08ba24d7428869328facc62345e47c87e03e7e8ff177408a9ace`. (확인)
- 필수 선행 검사: 최종 코드의 `cargo fmt --all -- --check` 통과, `cargo clippy --workspace --all-targets --locked -- -D warnings` 통과, `cargo audit` 통과(294개 의존성, advisory 1422개). Rust `1.97.1`, Cargo `1.97.1`, cargo-audit `0.22.2`, 기본 feature 및 호스트 target. Advisory DB `309392b18f6b17022f82c701f300433126901068`, 확인 시각 2026-11-08 03:32 UTC. Miri는 unsafe·메모리 안전성 전제 변경이 없어 비대상이며 Geiger는 실행하지 않았다. 이전 절의 최종 격리 PostgreSQL 테스트 결과를 같은 대상의 실행 근거로 사용했다. 전체 workspace 테스트는 마지막 테스트 보강 전 실행한 결과다. 선행 검사 통과 후 리뷰했다. (확인)
- 판정: **수정 필요, 78.75/100**. 실제 Refresh 잠금 경합의 검증 누락이 주요 발견 사항이며 점수와 무관하게 보완이 필요하다. (추론)
- 복구: 기존 테이블에 CHECK를 별도 적용하지 않은 현재 코드만의 롤백은 가능하다. 새 테이블 생성 또는 기존 테이블에 제약을 별도 적용한 뒤에는 코드 롤백만으로 DB 스키마·저장 데이터가 이전 상태로 돌아가지 않으므로 DB 복구·보정 계획이 필요하다. 운영 롤백은 실행하지 않았다. 아키텍처 책임과 배포 단위는 유지되며 auth에 저장 계약이 추가됐다. DB 적용을 포함하면 **R2**로 평가한다. (확인·추론)

| 항목 | 점수 / 4 | 근거 |
| --- | ---: | --- |
| 기능 | 3 | 해시 조회·폐기·제약과 트랜잭션 경계를 구현했으나 실제 Refresh 행 잠금 경합 검증이 빠졌다. |
| 가독성과 구조 | 4 | 저장과 업무 판단·트랜잭션의 책임이 분리되어 기존 구조를 따른다. |
| 안정성 | 3 | 롤백과 계정 경합 및 최초 폐기 보존을 검증했다. 별도 Refresh 행 경합 검증은 남았다. |
| 보안 | 3 | 원문을 저장 API·Debug에 노출하지 않고 해시를 사용한다. 후속 호출자의 잠금 순서와 정책 준수는 별도 검증이 필요하다. |
| 성능 | 4 | 해시 유일 인덱스와 계정·세션 인덱스를 사용한다. 처리량 측정은 하지 않았다. |
| 테스트 용이성 | 2 | 격리 DB와 명시적 시각을 사용하지만 핵심 행 잠금 제거를 감지하는 경합 테스트가 없다. |

리뷰만 수행했으며 코드와 테스트는 수정하지 않았다. 로그인·갱신 usecase, 실제 배포·기존 DB 변환 및 운영 롤백은 검토·실행 범위가 아니다. 기존 검증 결과는 행 잠금 자체의 경합까지 검증했다는 의미가 아니다. (확인)

### 리뷰 후 테스트 보완

위 리뷰의 Refresh 행 잠금 경합 검증 누락을 보완했다. 기존 단일 DB 테스트를 해시 조회·제약, 호출자 rollback, 폐기 범위·최초 결과 보존, 계정 잠금·재조회, Refresh 행 잠금의 다섯 독립 시나리오로 분리했다. 각 테스트는 고유 임시 스키마를 만들고 성공·panic 후 정리한다. (확인)

행 잠금 시나리오는 선행 트랜잭션이 계정과 Refresh 행을 잠근 상태에서 다른 트랜잭션이 같은 해시의 `lock_by_hash`를 호출한다. 행 잠금 자체를 검증하기 위해 테스트의 대기자만 계정 잠금을 생략한다. 대기 중 완료되지 않음을 확인하고 선행 트랜잭션에서 폐기·commit한 뒤 같은 대기 호출이 최초 폐기 시각·사유를 반환하는지 확인한다. 별도의 계정 잠금 시나리오는 실제 호출 순서를 계속 검증한다. (확인)

분리된 다섯 테스트를 실제 PostgreSQL 임시 스키마에서 병렬 실행해 **5 passed, 0 failed**. 명령은 `TLAB_TEST_DATABASE_URL=… cargo test -p tlab-app auth::repository::refresh_token --locked --offline -- --ignored`. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`도 통과했다. 코드·의존성은 바꾸지 않았다. 위 리뷰에서 지적한 테스트 누락은 해소되었으며, 위 점수와 판정은 보완 전 버전의 기록이다. 전체 workspace 테스트와 audit은 이번 테스트 전용 변경 후 재실행하지 않았다. (확인)

## 저장소 테스트 파일 통합

사용자 요청으로 `repository/refresh_token/tests.rs`를 `repository/refresh_token.rs`의 `#[cfg(test)] mod tests` 안으로 합쳤다. 저장소 구현·테스트 계약·모듈 경로는 유지한다. 과거 리뷰의 파일 경로는 당시 검토 대상이다. (확인)

파일 통합 후 관련 테스트 6개와 앱 전체 target clippy·fmt·diff 검사가 통과했다. 저장소 테스트는 실제 로컬 PostgreSQL에서 실행했다. (확인)

## 구현·변경 기록

### AUTH-01: 경합 완화와 고정 만료 계약 반영

사용자 요청으로 포괄적인 계정→Refresh 잠금 요구를 시나리오별 계약으로 수정했다. 갱신·로그아웃 경합을 허용하고 동일 Refresh의 재시도·반복 갱신이 최초 로그인부터 168시간인 Refresh·세션 만료를 연장하지 않음을 명시했다. 로그인 발급과 상태 변경·전체 폐기의 계정 잠금 전제는 유지한다. repository 조회 주석도 호출자가 정책을 소유하도록 맞췄다. 문서·주석만 변경했으며 소스 대조·diff 검사를 수행했다. 과거 검증·리뷰는 당시 기록으로 보존하고 실행 테스트는 다시 수행하지 않았다. (확인)

### DOC-01: 문서 채택 상태 정리

기존 저장 계약·구현 기록과 이번 사용자 반영 요청을 근거로 채택 상태를 accepted로 정리했다. implemented는 채택 상태와 구현 완료를 혼용하므로 제거했으며 실제 구현·검증 여부는 각 기록으로 유지한다. (확인)

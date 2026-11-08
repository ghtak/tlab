---
id: PLN-AUTH-JWT-REFRESH-LIFECYCLE
status: accepted
---

# Refresh 갱신·종료 구현과 검증

## 계약과 책임

갱신은 `database.conn()`으로 얻은 하나의 연결에서 Refresh 원문의 SHA-256으로 행을 조회하고 계정을 조회한다. 두 조회는 일반 SELECT이며 명시적 트랜잭션을 시작하지 않는다. 행 잠금 없이 읽은 계정 허용 상태와 미폐기 및 두 만료(`now < expires_at`, `now < session_expires_at`)를 발급 직전 현재 시각으로 확인한다. 서버에서 읽은 계정으로 `sub`와 `user`, Refresh 행으로 `sid`를 구성해 Access만 서명하고 서명 성공 후 반환한다. DB 변경과 명시적 commit은 없다. `exp = iat + 600`초다. Refresh·sid·저장 시각은 바꾸지 않아 반복·동시 갱신과 재시도가 가능하며 최초 로그인부터 168시간이 유지된다. 로그인과 갱신은 같은 허용 상태·user 구성 정책을 공유한다. (확인)

2026-11-08 사용자 합의로 갱신의 계정·Refresh 행 잠금과 종료에 대한 직렬화 요구를 완화했다. 읽기와 겹친 로그아웃 또는 상태 변경이 먼저 commit해도 진행 중인 갱신은 이전 상태로 Access를 발급할 수 있으며, 완료된 폐기를 이후에 조회한 갱신은 거부한다. 발급된 Access는 발급부터 600초 유효하다. 요청이 조회 이후 장시간 중단되었다가 재개되는 경우까지 종료 완료부터 정확히 10분으로 상한을 보장하지는 않는다. 두 조회는 같은 연결에서 각각 자동 커밋되는 별도 문장이므로 같은 시점의 스냅샷을 보장하지 않는다. (확인·추론)

로그아웃은 행을 잠그지 않는 원문 해시 조회로 로그인을 식별한 뒤 해당 로그인 미폐기 행을 조건부 UPDATE한다. 별도의 계정 잠금을 잡지 않으며 UPDATE에 필요한 PostgreSQL 행 잠금은 발생한다. 존재하지 않는 원문은 `InvalidRefresh`다. 만료되었거나 이미 폐기된 행도 해당 로그인 식별에는 사용할 수 있고 반복 로그아웃은 성공한다. 최초 폐기 시각·사유는 유지한다. 이 계약은 토큰 갱신 허용과 구분되며 HTTP 응답·쿠키 제거 방식은 정하지 않는다. (확인·추론)

계정 전체 폐기는 신뢰된 서버 코드가 계정 행을 잠근 트랜잭션에 `repository::refresh_token::revoke_account`를 직접 조합한다. 호출자가 상태 변경과 폐기를 같은 트랜잭션에서 완료해야 하며 임의 클라이언트 account_id에 종료 권한을 부여하지 않는다. 계정 정지 상태·탈퇴 기능은 추가하지 않았다. 기존 Access는 별도 DB 조회나 denylist 없이 만료까지 최대 10분 유지된다. (확인)

## 잠금 순서와 경계

로그인과 계정 상태 변경·전체 폐기의 조합은 같은 계정 잠금으로 직렬화한다. 일반 로그아웃은 계정 잠금을 잡지 않고 미폐기 조건부 UPDATE를 사용하며, 동시 폐기는 먼저 기록한 결과를 보존한다. PostgreSQL의 UPDATE 자체는 해당 행을 잠그므로 같은 행의 동시 변경은 대기할 수 있다. 여러 계정을 잠그는 후속 작업은 잠금 순서를 정해야 한다. 전체 폐기 함수의 계정 잠금 전제는 타입으로 강제하지 않으므로 호출자의 책임이다. 외부 분산 잠금은 추가하지 않았다. (확인·추론)

## 검증

- `env TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --locked --offline -- --include-ignored`: 최초 구현에서 39 passed. 잠금 제거 후 결과는 아래에 별도 기록한다. 각 DB 시나리오는 고유 임시 스키마에 migration을 적용하고 정리했다. 실제 PostgreSQL과 Ed25519를 사용했다. 운영·기존 사용자 스키마는 변경하지 않았다. (확인)
- 반복·동시 갱신의 저장 행 전체 불변, sid·sub·최신 nickname·600초, 7일 전후, 각각의 만료 판정, 미존재·폐기·탈퇴 거부, 만료·반복 로그아웃, 기기 격리와 최초 폐기 보존, 다른 계정 보존, 전체 폐기 및 상태 변경과 rollback을 확인했다. 두 만료를 독립적으로 검사하는 테스트는 현재 스키마가 금지하는 조합을 만들기 위해 해당 테스트 전용 스키마에서만 두 CHECK를 제거한다. 7일 전후는 경계 양쪽 1분을 사용했고 정확한 동일 시각은 직접 재현하지 않았다. (확인)
- 최초 잠금 기반 구현에서 계정 잠금 대기 중 폐기 후 재조회, 실제 동시 갱신·로그아웃, 잠금 대기 중 만료, 잘못된 실제 서명 키의 실패, 폐기 SQL 실패와 지연 트리거에 의한 commit 실패·rollback을 확인했다. 갱신의 commit 실패 자체는 별도로 주입하지 않았다. (확인)
- `cargo test --workspace --locked --offline`: 통과. 앱의 마지막 테스트 두 개 추가 전에 실행했으며 기본 실행에서 DB ignored 테스트는 제외된다. 위 앱 전체 테스트는 최종 추가를 포함한다. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`, `cargo audit` 통과. audit은 1422 advisory와 294개 의존성을 검사했다. (확인)

HTTP/API 응답과 Bearer 및 시작 시 키 조립은 [JWT API](../jwt-api/plan.md)에 연결했다. 쿠키·CSRF·JWKS와 실제 상태 변경 usecase 연결은 후속 범위다. 테스트용 signer·clock 주입 경계를 운영 코드에 추가하지 않았다. (확인)

## 갱신 잠금 제거 후 검증

미커밋 로그아웃이 계정과 Refresh를 잠근 동안에도 갱신이 완료되고, 로그아웃 commit 이후 새 갱신이 거부되는 경합 테스트로 변경했다. 발급된 Access는 공개키로 확인한다. 기존 만료·폐기·서명 실패·rollback 테스트도 유지한다. (확인)

변경 후 `env TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --locked --offline -- --include-ignored`: 38 passed. `cargo test --workspace --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`: 통과. workspace의 ignored DB 테스트는 별도 실행한 앱 테스트 외에는 실행하지 않았다. `cargo audit`은 잠금 제거 전에 통과했고 의존성은 변경하지 않았다. (확인)

## 로그아웃 잠금 제거

사용자 합의에 따라 로그아웃의 선행 계정·Refresh 행 잠금을 제거했다. 미폐기 조건부 UPDATE는 최초 폐기를 보존하고, 갱신의 읽기와는 직렬화하지 않는다. 상태 변경과 전체 폐기는 호출자가 계정 잠금을 소유하는 트랜잭션에서 조합한다. (확인)

로그아웃 변경 후 격리 PostgreSQL 앱 전체 테스트 39개 통과. 계정 행을 다른 트랜잭션이 잠근 동안 로그아웃이 완료되는지 확인했다. workspace clippy, fmt, `git diff --check` 통과. workspace 전체 회귀는 이 변경 직전 갱신 잠금 제거 후 통과했고, audit은 그 이전에 통과했으며 의존성은 변경하지 않았다. (확인)

## 코드 리뷰

- 대상: `HEAD bf458a105323e14d9290c61392e6c638586a4848` 대비 미커밋 변경. 핵심 파일 SHA-256: lifecycle `0f5f870183cb187c7ab75816759d2e14757f6d8ae12959f4fc9f7ba37a54c692`, 테스트 `b7348bdb597ea053302533b8b48962b0e7af7bdd83f99323158117aabd1ab8da`, 공용 정책 `d95e23dd7eabf3c5e8a16c4b2e8c0778c7d9e7a59a0cd43a85255bb8e68e34c2`. (확인)
- 선행 검사: Rust 1.97.1, Cargo 1.97.1, 기본 feature·호스트 target에서 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo audit` 모두 통과 후 리뷰했다. audit 0.22.2, 294 의존성·1422 advisory, DB `309392b18f6b17022f82c701f300433126901068`, 2026-11-08 04:04 UTC. unsafe·메모리 안전성 전제 변경이 없어 Miri 비대상, 의존성·unsafe 현황 추가 조사 필요가 없어 Geiger 미실행. 앞 절의 동일 코드 PostgreSQL 앱 39개 및 변경 직전 workspace 테스트를 참조했다. (확인)
- 판정: **수정 필요, 75/100**. 경합 테스트의 실패 판정 누락과 문서 간 잠금 계약 불일치가 발견되었다. (추론)

### 발견 사항

1. **[주요: 검증 누락]** `tlab-app/src/auth/refresh_lifecycle/tests.rs:319`의 동시 갱신 테스트는 `Ok`만 검사하고 모든 `Err`를 허용한다. 갱신이 DB 오류 등 `Internal`을 반환해도 이후 로그아웃과 재갱신 확인이 성공하면 테스트가 통과한다. 합의한 결과는 유효 Access 또는 폐기를 관찰한 `InvalidRefresh`이므로 이 두 경우만 명시적으로 허용해야 한다. 별도 비차단 테스트는 로그아웃 commit 전에 갱신을 완료하므로 commit 후 반환하는 겹침 자체는 재현하지 않는다. 구현 결함의 직접 재현이 아니라 회귀 검출 누락이다. (확인·추론)
2. **[경미: 계약 문서 불일치]** `tlab-app/migrations/0002_create_tlab_auth_refresh_token.sql:11` 및 `:90`은 갱신·로그아웃을 포함한 모든 경로가 계정→Refresh 잠금을 수행한다고 설명한다. 현재 합의·구현·이 문서는 두 경로가 선행 잠금 없이 진행한다고 명시한다. SQL 주석과 DB 컬럼 설명이 향후 호출자에게 잘못된 직렬화 보장을 전달하므로 실제 계약과 일치시켜야 한다. 이 리뷰에서는 수정하지 않았다. (확인·추론)

| 항목 | 점수 / 4 | 근거 |
| --- | ---: | --- |
| 기능 | 3 | 발급·만료·폐기·기기 격리와 기존 로그인 계약 확인. 경합 결과 검증에 제한 |
| 가독성과 구조 | 3 | 공용 계정 정책과 시나리오 분리. SQL 문서의 기존 잠금 계약과 불일치 |
| 안정성 | 3 | 실패·rollback과 최초 폐기 보존 검증. 동시 갱신의 예기치 않은 오류는 테스트가 놓침 |
| 보안 | 3 | 원문 해시 조회와 서버 계정 사용, 합의된 경합 허용. 지연 중 갱신 반환에는 종료 시점 기준 엄격한 10분 상한 없음 |
| 성능 | 4 | 갱신의 행 잠금 없음, 해시 인덱스 조회. 처리량 벤치마크는 미실행 |
| 테스트 용이성 | 2 | 격리 DB와 실제 서명·실패 재현은 있으나 주요 경합 결과 분기가 과도하게 허용됨 |

가중 점수 `18.75 + 11.25 + 15 + 15 + 10 + 5 = 75`. (추론)

복구·아키텍처: auth에 갱신·폐기 계약이 추가되어 경계·흐름이 바뀐다. 현재 HTTP에 연결되지 않은 코드만 되돌리기는 가능하나 연결 후 실제 폐기된 Refresh는 코드 롤백만으로 복구되지 않는다. 상태 복원을 위해 폐기를 되돌리는 것은 보안상 별도 판단이 필요하고 재로그인이 대안이다. 배포 후 데이터 효과를 포함한 보수적 등급은 **R3**이며 실제 배포·롤백은 검증하지 않았다. 계정 상태 변경 usecase·HTTP 연결 및 엄격한 경과 시간 상한은 후속 또는 명시적으로 완화된 범위다. (확인·추론)

### 리뷰 후 보완과 재확인

동시 갱신 테스트가 성공 또는 `InvalidRefresh`만 허용하고 `Internal`은 실패시키도록 수정했다. 별도 격리 DB 테스트는 계정 테이블 잠금과 PostgreSQL 차단 관계를 이용해 Refresh 조회 이후 계정 조회가 대기 중임을 확인하고, 로그아웃 commit 후 대기를 해제해 진행 중 갱신의 Access 발급과 후속 갱신 거부를 확인한다. SQL 주석·컬럼 설명도 현재 잠금 계약에 맞췄다. 기존 리뷰의 두 발견 사항은 해소되었다. (확인)

변경 후 격리 PostgreSQL `cargo test -p tlab-app auth::refresh_lifecycle --locked --offline -- --ignored` 12개 통과, fmt·workspace 전체 대상 clippy·audit·`git diff --check` 통과. workspace 전체 테스트는 이번 테스트·SQL 주석 보완 후 재실행하지 않았다. 수정 범위를 재검토했으며 새 결함은 발견하지 못했다. 기존 점수와 판정은 수정 전 대상의 기록이다. 운영 배포·복구는 검증하지 않았고, 경합 허용 및 실행이 오래 중단될 때 종료 시점 기준 엄격한 10분 상한이 없다는 한계는 그대로다. (확인·추론)

## 구현·변경 기록

### 갱신·로그아웃 slice 분리

사용자가 제안된 분리를 요청하여 `auth/slice/refresh_access_token/`과 `auth/slice/logout/`으로 각각 route·usecase·테스트를 배치했다. 기존 `RefreshLifecycle`과 공용 오류 타입을 제거하고 각 usecase가 자신의 오류를 소유한다. `revoke_all_locked`는 `auth/session.rs`에서 제공하며, 계정 잠금과 호출자 트랜잭션의 전제는 유지한다. 테스트의 세션·키 준비는 `auth/test_support.rs`에서 공유한다. HTTP 경로·상태·본문·no-store 및 갱신·폐기 계약은 유지한다. 로그의 operation 값은 각 시나리오 이름으로 구분한다. (확인)

두 slice의 usecase 진입점은 사용자 요청에 따라 `execute`로 통일했다. route에서 `usecase::execute`를 호출한다. (확인)

검증 중 기본 테스트 23개와 DB 테스트 28개를 각각 실행해 통과했다. `execute` 명칭 반영 후 clippy(앱 전체 target)·fmt·diff 검사가 통과했다. 최종 전체 병렬 실행은 50개 통과·1개 실패였으며, 기존 로그인 테스트의 시간 기반 임시 스키마 이름이 충돌했다(`pg_namespace_nspname_index`). 샌드박스의 로컬 DB 연결 제한은 권한을 부여한 재실행으로 해소했다. (확인)

최종 `cargo test -p tlab-app --locked --offline -- --include-ignored --test-threads=1`을 로컬 PostgreSQL 연결 권한과 함께 실행하여 51개 모두 통과했다. 자체 점검에서 기존 처리·트랜잭션·오류 응답과 시나리오 간 경합 검증이 유지됨을 확인했다. 기존 병렬 테스트의 임시 스키마 이름 충돌은 이번 변경 범위에서 수정하지 않았다. (확인)

### 계정 전체 폐기 중계 함수 제거

사용자 요청에 따라 저장소 호출만 전달하던 `revoke_all_locked`와 `auth/session` 모듈을 제거했다. 호출자는 `repository::refresh_token::revoke_account`를 직접 사용하며, 같은 트랜잭션에서 계정 행을 먼저 잠그는 책임은 유지한다. 상태 변경과 전체 폐기의 rollback 테스트는 refresh_token 저장소 테스트로 옮겼고 로그아웃과 전체 폐기 조합 테스트도 직접 호출하도록 변경했다. 앞선 slice 분리 기록의 session 모듈 배치는 이 변경으로 대체된다. (확인)

중계 제거 후 기본 앱 테스트 23개, 상태 변경·폐기 rollback 및 로그아웃·전체 폐기 조합 DB 테스트 각 1개가 통과했다. 앱 전체 target clippy·fmt·diff 검사도 통과했다. (확인)

### 갱신 트랜잭션 제거 코드 리뷰

- 범위: 직전 대화의 트랜잭션 기반 execute 대비 사용자가 수정한 `auth/slice/refresh_access_token/usecase.rs`. 현재 HEAD는 `b3dab1f`, 대상 파일은 미추적 상태이며 SHA-256 `10fdcb9e21c33df72e008dd688c3abccea7e1e25e92e00106846d3d53002e38c`를 기준으로 검토했다. 앞선 slice 이동 전체에 대한 재리뷰는 아니다. (확인)
- 선행 검사: Rust/Cargo 1.97.1, 기본 feature·호스트 target. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo audit` 통과 후 LLM 리뷰 진행. audit 0.22.2, 2026-11-08 실행, advisory DB `309392b18f6b17022f82c701f300433126901068`, 1422 advisory·294 의존성 검사. unsafe·메모리 안전성·동시성 구현 전제 변경이 없어 Miri 비대상이며, 의존성·unsafe 현황 조사 필요가 없어 Geiger 미실행. (확인·추론)
- 발견 사항 [경미]: 이 문서 계약과 책임 절은 여전히 트랜잭션 조회·commit 후 반환을 명시하여 현재 구현과 다르다. 일반 연결에서 조회·판정·서명 후 반환하는 것으로 설명 갱신이 필요하다. 리뷰 범위에서 소스와 해당 계약 본문은 수정하지 않았다. (확인)
- 코드에서는 수정이 필요한 결함을 발견하지 못했다. 하나의 연결로 잠금 없는 두 조회를 수행하고 계정 정책·폐기·두 만료·서명 오류 처리를 유지한다. 기존에 허용한 진행 중 갱신과 로그아웃 경합 조건도 유지한다. (확인·추론)
- 검증: 로컬 PostgreSQL 권한으로 `cargo test -p tlab-app --locked --offline auth::slice::refresh_access_token -- --ignored --test-threads=1` 8개 및 `cargo test -p tlab-app --locked --offline login_refresh_logout_http_flow -- --ignored` 1개 통과. workspace 전체 테스트·운영 배포·실제 롤백·성능 부하 측정은 실행하지 않았다. (확인)
- 품질 점수: 기능 4/4, 가독성과 구조 3/4(계약 문서 불일치), 안정성 4/4, 보안 4/4, 성능 4/4(BEGIN·COMMIT 제거, 처리량 개선은 미측정), 테스트 용이성 4/4(기존 실제 DB 경합·실패 검증 유지). 가중 합계 96.25/100, 판정 통과(문서 보완 권고). (추론)
- 리스크 R1: 책임·공개 계약·데이터 소유권 변경 없이 불필요한 명시적 트랜잭션만 제거한다. 데이터·스키마·설정 보정 없이 코드 복원으로 기존 경로로 돌아갈 수 있는 것으로 판단한다. 실제 롤백은 미검증이다. (추론)

### 갱신 트랜잭션 제거 문서 반영

사용자 요청으로 계약과 책임 절을 현재 구현에 맞췄다. 하나의 일반 연결에서 Refresh·계정을 조회하고 정책·폐기·만료를 검사한 뒤 Access 서명 성공 시 반환한다. 명시적 트랜잭션·commit 설명을 제거하고 두 조회의 스냅샷 경계도 명확히 했다. 위 코드 리뷰의 문서 불일치 항목은 해소되었으며, 이전 구현·검증 기록은 당시 상태를 나타내므로 보존한다. 문서 변경만 수행했고 본문·소스 대조와 `git diff --check`를 확인했다. (확인)

### DOC-01: 문서 채택 상태 정리

본문에 기록된 사용자 합의와 이번 DOC-01 반영 요청을 근거로 채택 상태를 accepted로 정리했다. 구현·과거 검증 결과는 채택 상태와 구분해 보존한다. 현재 lifecycle의 경합 완화와 고정 세션 만료 계약은 변경하지 않았다. (확인)

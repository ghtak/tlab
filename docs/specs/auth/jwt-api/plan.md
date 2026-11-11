---
id: PLN-AUTH-JWT-API
status: accepted
---

# JWT API 계약과 검증

앱이 시작할 때 `auth.jwt`의 Ed25519 키 쌍을 읽고 검증해 발급기와 공개키 검증기를 준비한다. `private_key`, `public_key`, `issuer`, `audience`가 필요하며 `generate_if_missing`은 기본 false다. 누락·불일치·잘못된 키나 빈 issuer/audience는 시작 실패다. 경로는 작업 디렉터리 기준이다. 검증기는 개인키·DB를 소유하지 않는다. (확인)

| 메서드·경로 | JSON 요청 | 성공 응답 |
| --- | --- | --- |
| POST `/api/v1/auth/local` | `email`, `password` | 200: `account_id`, `status`, `access_token`, `refresh_token` |
| POST `/api/v1/auth/refresh` | `refresh_token` | 200: `access_token`만 반환, 기존 Refresh 유지 |
| POST `/api/v1/auth/logout` | `refresh_token` | 200: `{}` |

토큰 응답과 오류는 `Cache-Control: no-store`다. Refresh 미존재·갱신 불가 시 401 `INVALID_REFRESH_TOKEN`; 기존 만료·폐기 행의 반복 로그아웃은 성공한다. 잘못된 JSON은 공통 오류 계약을 따른다. 정상 전달 계약인 Authorization 헤더의 Access와 JSON 본문의 Refresh를 별도 로그 필드로 기록하지 않는다. 공통 요청 로그는 전체 URI를 기록하며, query에 잘못 전달한 토큰의 로그 노출 방지는 사용자 결정에 따라 이번 범위에서 제외한다. (확인)

핸들러는 `claims: AccessClaims`로 인증하며 `Json`보다 앞에 둔다. `FromRef`로 공개키 검증기만 가져오고 요청별 DB·파일·네트워크 접근 없이 서명, EdDSA, iss/aud, 필수 클레임과 시간 정책을 검증한다. `Authorization`은 정확히 한 헤더여야 하며 대소문자 무관 `Bearer`, 단일 공백, 공백·쉼표 없는 토큰만 받는다. 누락·형식·검증 실패는 모두 401 `INVALID_ACCESS_TOKEN`, `WWW-Authenticate: Bearer`다. 계정 조회와 권한 판단은 호출자가 명시적으로 담당한다. 쿠키 fallback은 없다. (확인)

Refresh는 최초 로그인부터 7일 유지하며 갱신 시 sid를 보존하고 Access는 600초 유효하다. 동시 로그아웃과 겹친 갱신은 Access를 발급할 수 있고 종료 후에도 기존 Access는 만료까지 허용된다. `user`에는 서버의 nickname과 status만 들어간다. (확인)

## 검증과 검토

- DB 없는 실제 Router: claims와 JSON 동시 추출, 정상 인증, 누락·형식·중복 헤더, 잘못된 알고리즘·서명·issuer·audience·클레임·만료 거부. 기존 토큰 단위 테스트는 상세 필수 필드와 경계를 검증한다. (확인)
- 격리 PostgreSQL과 Router: 로그인 → 토큰 본문 → 보호 경로 → 갱신 및 sid 유지 → 로그아웃 → 갱신 거부 → 기존 Access 허용. 각 시나리오는 고유 임시 스키마를 사용한다. (확인)
- 초기 DB 실행에서 HTTP 로그인이 추가한 세션을 기존 테스트의 행 개수 기대값에 반영하지 않아 실패했고, 수정 후 앱 DB 포함 41개가 통과했다. (확인)
- 코드 검토에서 변조 테스트가 base64 마지막 문자에 의존하면 동일한 바이트가 될 수 있는 점을 발견해 서명 첫 바이트를 바꾸도록 보완했다. 테스트 전용 signer·clock 경계는 추가하지 않았다. (확인)
- 최종 `cargo test --workspace --locked --offline` 통과(기본 실행의 DB ignored 제외), `env TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --locked --offline -- --include-ignored` 42개 통과. 라이브러리 PostgreSQL `sqlxdb` 4개와 `migration` 1개도 별도 실행해 통과했다. 임시 스키마 또는 세션 임시 테이블을 사용했다. (확인)
- 최종 `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check` 통과. `cargo audit` 통과(294 의존성, 1422 advisory); 최종 테스트 보완 전 실행했으며 의존성 변경은 없다. 실제 배포·브라우저·JWKS·키 교체는 실행하지 않았다. (확인)

HTTP 핸들러와 요청·오류 변환을 `auth/refresh_lifecycle/route.rs`로 이동하고 `auth/route.rs`에는 등록만 남겼다. 이동 후 앱 기본 테스트, 격리 PostgreSQL 로그인·갱신·종료 Router 테스트, 앱 clippy, fmt·diff 검사가 통과했다. (확인)

## 코드 리뷰

- 대상: `HEAD 7803584c6874c16928bec927860690c22ea48f81` 대비 추적·미추적 작업 변경. 주요 SHA-256: extractor `d7572a144bebf69d7d825b55e0b96fec4d088b8b779c7027eb2743e6700aa71a`, lifecycle HTTP `d344ee5d4043eb41c5731bb09105e7fc7c0b3d3bb25dbe85f7d4d0e48a0af0a3`, app `0c6fd5e875cedc26a0e869a4e79c2aa397785cf11a2d472f0f6084c9cb999f6a`. (확인)
- 선행 검사: Rust 1.97.1, Cargo 1.97.1, 기본 feature·호스트 target에서 fmt, workspace 전체 대상 clippy(`--locked --offline`, 경고 거부), audit 0.22.2 통과. audit 294 의존성, 1422 advisory, DB `309392b18f6b17022f82c701f300433126901068`, 2026-11-08 04:23 UTC. unsafe·메모리 안전 전제 변경이 없어 Miri 비대상, unsafe·의존성 현황 추가 자료가 불필요해 Geiger 미실행. (확인)
- **판정: 수정 필요, 73.75/100.** 아래 발견은 코드 경로 및 테스트 조건에서 도출했으며 별도 공격 재현은 하지 않았다. (추론)

1. **[주요: 민감정보 노출]** `tlab/src/http/trace.rs:35`의 기존 공통 요청 span은 전체 URI를 로그에 남긴다. 새 인증 API에서도 호출자가 실수로 `?refresh_token=<원문>` 또는 Access를 query에 넣으면, 정상 인증에 사용되지 않더라도 라우팅 전에 원문이 로그에 기록된다. 헤더·JSON 정상 계약은 해당하지 않지만 원문 로그 미노출 요구에는 빈틈이다. 인증 경로의 query 제거 또는 안전한 공통 URI 기록 정책과 로그 테스트가 필요하다. (확인·추론)
2. **[주요: 검증 누락]** `tlab-app/src/auth/extractor.rs:124` 만료 사례는 `iat=now`, `exp=now-1`로 600초 수명 조건도 위반한다. `now < exp` 검사가 제거되어도 다른 조건 때문에 401이므로, extractor Router에서 정상 수명을 가진 만료 JWT가 거부되는지 입증하지 못한다. `iat=now-601`, `exp=now-1`처럼 다른 조건을 만족하는 서명 토큰으로 확인해야 한다. 토큰 단위 테스트의 만료 경계는 별도로 존재한다. (확인·추론)

| 항목 | 점수 / 4 | 근거 |
| --- | ---: | --- |
| 기능 | 3 | 정상 API 연결과 상태 계약은 확인. 만료 Router 테스트의 독립성 부족 |
| 가독성과 구조 | 4 | 핸들러와 usecase, verifier 의존성 경계 분리 |
| 안정성 | 3 | 트랜잭션과 경합 계약 확인. 배포·복구 미실행 |
| 보안 | 2 | 메모리 검증과 고정 오류는 적절하나 query에 포함된 원문이 공통 로그에 노출될 수 있음 |
| 성능 | 4 | 보호 요청은 DB·파일 I/O 없이 검증. 처리량 측정은 미실행 |
| 테스트 용이성 | 2 | DB 없는 Router와 격리 DB를 사용하나 만료 실패 조건이 중첩됨 |

가중 합계 `18.75 + 15 + 15 + 10 + 10 + 5 = 73.75`점. (추론)

복구 가능성은 단방향, 구조 변경 있음으로 **R3**다. 로그인 응답과 시작 설정이 바뀌고 로그인·로그아웃이 Refresh 행을 생성·폐기한다. 코드 롤백은 이전 인증 계약으로 돌리지만 이미 폐기된 Refresh나 배포된 토큰의 상태를 복원하지 못한다. 클라이언트·설정 전환과 재로그인 등 복구 계획이 필요하며 실제 배포·롤백은 검증하지 않았다. 운영 환경에서 단계적 적용과 오류율 관찰, 중단 기준도 미확인이다. (확인·추론)

### 리뷰 후 처리

사용자 판단에 따라 쿼리로 전달한 토큰의 로그 노출 방지는 이번 API 계약 범위에서 제외했다. 정상 계약은 Access를 Authorization 헤더로, Refresh를 JSON 본문으로 전달하며 기존 전체 URI 기록 동작은 유지한다. (확인)

만료 Router 테스트는 `iat=now-601`, `exp=now-1`로 변경해 600초 수명과 나머지 조건을 만족하는 서명 토큰을 사용한다. `cargo test -p tlab-app auth::extractor::tests::bearer_contract_without_database --locked --offline` 및 fmt 검사 통과. 기존 리뷰 점수와 판정은 수정 전 대상의 기록이다. (확인)

## 구현·변경 기록

### AUTH-04: 토큰 로그 비기록의 적용 범위 명시

사용자 요청으로 정상 헤더·본문 전달의 토큰을 별도 기록하지 않는다는 계약과 전체 URI 로그의 query 노출 한계를 본문에서 구분했다. 기존 사용자 결정을 반영한 문서 변경이며 로깅 동작은 변경하지 않았다. 본문·소스 대조와 diff 검사를 수행했고 실행 테스트는 다시 수행하지 않았다. (확인)

### DOC-01: 문서 채택 상태 정리

기존 API 연결·사용자 결정 기록과 이번 DOC-01 반영 요청을 근거로 채택 상태를 accepted로 정리했다. 구현·검증 완료 여부와 채택 상태를 구분하고 과거 리뷰·수치는 당시 대상으로 보존한다. (확인)

### 선택적 인증 변경: Clippy 수정과 재리뷰

- 대상: `HEAD 2ee0adae278790318337d2bbe4363604b39a18b6` 대비 미커밋 `auth/extractor.rs`, `auth/route.rs`, `main.rs`, 신규 `restclient/temp.http`. 세 Rust 파일 diff의 SHA-256은 `886e918c21b9d8c7a0632d297562cbd8825fd5b2bfcd41d7e041dc51f423c7af`다. 리뷰 중 main의 JSON stderr 출력이 복원되어 최신 소스를 기준으로 재검사했다. (확인)
- 최초 필수 Clippy 검사에서 `verified_claims`의 큰 `Response` 오류 반환으로 `clippy::result_large_err`가 발생했다. 사용자 수정 요청에 따라 helper는 기존 `TokenError`를 그대로 반환하고 요청 추출기가 HTTP 500으로 변환하도록 변경했다. 기존 오류 분류 테스트는 내부 오류 종류 보존을 확인하도록 조정했다. 선택적 인증의 성공·미인증 처리와 필수 인증의 401 응답은 유지한다. 이번 코드 수정 범위는 extractor다. (확인)
- Rust/Cargo 1.97.1, Clippy 0.1.97, 기본 feature·host target에서 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test -p tlab-app auth::extractor::tests --locked` 통과. 테스트 **4 passed, 0 failed**로 정상·잘못된·만료 토큰, 선택적 인증, 비신뢰 extension, 내부 오류 분류를 확인했다. `git diff --check`도 통과했다. unsafe·메모리 안전 전제 변경이 없어 Miri 비대상이며 의존성·unsafe 현황 추가 자료가 필요하지 않아 Geiger는 미실행했다. (확인)
- `cargo audit` 0.22.2는 종료 코드 1. 2026-11-11 실행, advisory DB `fd6f39814c4d3c359fdbe223d0280dec4933f78f`, 1458 advisories·306개 의존성에서 기존 `libsqlite3-sys 0.37.0`의 `RUSTSEC-2026-0459` 1건을 보고했다. 도구 제시 수정 버전은 `>=0.39.0`이며 이번 변경에 의존성·잠금 파일 변경은 없다. audit 결과는 선행 검사 중단 조건이 아니므로 기록하고 LLM 리뷰를 진행했다. (확인)

**판정: 수정 필요, 71.25/100.** Clippy 문제는 해결했으나 전체 미커밋 변경에는 아래 별도 결함이 남는다. (확인·추론)

1. **[주요/P2] `tlab-app/src/main.rs:23`: bootstrap CLI가 tracing 준비에 의존한다.** tracing 초기화가 CLI 분기 앞으로 이동해 bootstrap도 설정된 로그 디렉터리를 만들고, 로그 출력 경로 권한이나 필터 설정 문제로 업무 처리 전에 실패할 수 있다. 이는 [기존 CLI 테스트](../../../../tlab-app/tests/bootstrap_admin_cli.rs)의 추가 파일 없음 계약과 [RBAC 설계](../../rbac/manage-access/plan.md)의 tracing 미초기화 계약에 어긋난다. CLI의 JSON stderr 출력은 리뷰 중 복원되어 출력 대체 문제는 현재 결함으로 다루지 않는다. tracing 초기화를 bootstrap 분기 뒤로 되돌리는 등 CLI에 필요한 초기화만 수행하도록 조정해야 한다. (확인: 코드·기존 계약 / 추론: 권한·설정 오류 영향·수정 방향)

   최신 소스에서 `cargo build -p tlab-app --locked` 후, 기본 설정·임시 cwd에서 UTF-8이 아닌 stdin 한 줄을 전달해 DB 연결 전 입력 실패를 재현했다. 종료 코드 1, stdout 0바이트, stderr 73바이트·JSON 파싱 성공, `logs` 디렉터리 생성을 확인했다. JSON 출력 복원은 확인했지만 추가 파일 없음 계약 위반은 남는다. 기존 CLI DB 테스트 전체는 실행하지 않았다. (확인)

2. **[주요/P2] `tlab-app/src/auth/route.rs:28–36`: 새 `/api/v1/auth/me`가 공통 인증 실패 응답을 우회한다.** 토큰 누락·변조·만료에서 선택적 추출기는 `None`을 반환하고 handler는 빈 401만 반환한다. 이 문서의 `INVALID_ACCESS_TOKEN` JSON·`WWW-Authenticate: Bearer` 계약이 이 보호 API에는 적용되지 않는다. 익명 응답이 필요한 별도 동작이 없으므로 필수 `AccessClaims` 추출기를 사용하면 기존 인증 실패 계약을 재사용할 수 있다. handler 경로를 실제 요청으로 실행한 재현은 하지 않았으며, 코드 흐름과 기존 필수 추출기 Router 테스트를 근거로 판단했다. (확인: 코드·계약 / 추론: 영향·수정 방향)

| 항목 | 점수 / 4 | 근거 |
| --- | ---: | --- |
| 기능 | 2 | 선택적·필수 추출기 회귀는 통과했으나 CLI 초기화와 새 보호 API의 실패 계약이 어긋남 |
| 가독성과 구조 | 3 | 토큰 오류 분류와 응답 변환을 분리함. route 등록 파일에 개별 me 응답 처리도 포함됨 |
| 안정성 | 3 | JSON CLI 오류 출력은 복원됨. bootstrap 실행이 추가로 tracing 준비에 의존함 |
| 보안 | 3 | 기존 서명 검증·필수 인증 거부·비신뢰 extension 차단은 유지. 새 me의 공통 인증 실패 응답 보완 필요 |
| 성능 | 4 | 요청별 DB·파일 접근 없는 검증을 유지하며 추가 네트워크·쿼리 없음. 부하 성능 측정은 미실행 |
| 테스트 용이성 | 3 | DB 없는 Router 테스트와 기존 별도 CLI 프로세스 테스트를 활용 가능. 새 me 경로 검증은 미실행 |

- **복구 가능성: PR 롤백만으로 기존 실행 동작 복구 가능, 아키텍처 변경 있음, R2.** 이번 diff는 스키마·업무 데이터 쓰기·토큰 발급 형식을 바꾸지 않는다. 공용 인증 추출기의 선택적 계약, 새 공개 조회 경로, bootstrap의 초기화 생명주기가 바뀐다. 코드 복원으로 기존 계약에 돌아갈 수 있으나 생성된 로그 파일은 남으며 새 me 호출자는 경로 제거의 영향을 받는다. 운영 전환은 호출자 사용 여부와 CLI 실행 환경을 확인해야 한다. 실제 운영 배포·롤백은 수행하지 않았다. (확인: 변경 범위 / 추론: 복구 조건·등급)
- **미확인 범위:** 전체 workspace 테스트·DB 포함 CLI 테스트·웹 쿠키 경로 재실행·새 me의 실제 Router 요청·부하·운영 전환 및 복구. 기존 검증 기록을 이번 버전의 통과 근거로 대체하지 않았다. 신규 REST Client 파일의 고정 토큰은 수동 요청 자료이며 자동 검증 결과로 취급하지 않았다. (확인)

### 후속 수정: CLI 초기화 복구와 선택적 인증 확인 라우트

- 사용자 설명에 따라 `/api/v1/auth/me`는 `Option<AccessClaims>` 동작을 확인하는 라우트로 유지한다. 앞선 리뷰에서 필수 추출기로 바꾸도록 제안한 판단은 의도를 잘못 해석했으므로 철회한다. 유효한 토큰은 `Some`으로 받아 sid/sub를 반환하고, 누락·무효·만료는 `None`으로 받아 공통 JSON 401과 `WWW-Authenticate: Bearer`를 반환한다. 토큰 검증의 내부 오류는 선택적 추출기에서 500으로 전달한다. (확인: 사용자 설명·구현)
- bootstrap CLI 분기 뒤로 tracing 초기화를 복원했다. 초기 관리자 CLI는 기존 JSON 오류 출력을 유지하며 tracing 파일·TLS/JWT 키·서버를 준비하지 않는다. main은 HEAD와 동일하다. (확인)
- 새 Router 테스트 `auth::route::tests::me_maps_optional_authentication_to_identity_or_common_error`는 실제 handler와 공개키 검증기를 연결해 유효 토큰의 sid/sub, 누락·무효·만료 토큰의 공통 401 본문·인증 헤더를 확인한다. 기존 선택적 추출기 테스트는 `None`이 handler에 전달되는 계약을 별도로 검증한다. (확인)
- Rust/Cargo 1.97.1, Clippy 0.1.97, 기본 feature·host target에서 fmt·workspace 전체 대상 Clippy(`--locked`, 경고 거부)를 다시 통과했다. 필수 추출기를 사용했던 중간 수정 시점의 앱 기본 검사에서 34개와 별도 RBAC 테스트 13개가 통과했으며, DB 필요 사례는 기본 실행에서 제외됐다. 해당 중간 결과를 최종 선택적 라우트 검증으로 사용하지 않는다. (확인)
- `env TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --test bootstrap_admin_cli --locked -- --include-ignored`: **1 passed, 0 failed**. 격리 스키마와 별도 프로세스에서 CLI 성공·실패·충돌의 JSON 출력, 비밀 비노출, 로그·키 파일 미생성, 저장 원자성을 확인했다. 이 실행 이후 CLI 코드는 변경하지 않았다. (확인)
- audit은 앞선 동일 세션 결과를 재사용한다. 의존성·잠금 파일·도구 설정 변경이 없으며 기존 `RUSTSEC-2026-0459` 1건은 미해결 상태다. Miri·Geiger의 비대상 사유도 동일하다. (확인)
- 최종 `cargo test -p tlab-app auth:: --locked`: **23 passed, 0 failed, 44 ignored**. 선택적 me Router·선택적/필수 추출기·토큰·DB 없는 auth 회귀를 포함한다. DB 필요 auth·웹 사례는 이번 명령에서 실행하지 않았다. `git diff --check`도 통과했다. (확인)

**최종 재리뷰: 이번 변경에서 추가 결함 없음, 통과, 91.25/100, R2.** 앞선 CLI 초기화 결함은 수정했고, me의 필수 추출기 변경 제안은 사용자 의도에 맞춰 철회했다. 최종 세 Rust 파일 diff SHA-256은 `237fa80242c5f2da967598671ed364a2b3657a14c216aef8276462c97d7f9cd2`다. 기존 의존성 advisory는 이 판정과 별도로 남긴다. (확인·추론)

| 항목 | 점수 / 4 | 최종 근거 |
| --- | ---: | --- |
| 기능 | 4 | 선택적 인증 확인 의도와 Some/None 응답 연결, CLI 초기화·출력·저장 계약을 검증 |
| 가독성과 구조 | 3 | 토큰 오류 분류·HTTP 변환 책임을 구분함. 확인용 handler·테스트는 route 등록 파일에 함께 위치 |
| 안정성 | 4 | CLI의 tracing 의존 제거, 내부 검증 오류 전달과 기존 bootstrap 원자성 테스트 통과 |
| 보안 | 3 | 서명 검증·필수 인증·비신뢰 extension 차단 유지. 기존 의존성 advisory 1건은 미해결 |
| 성능 | 4 | 요청별 DB·파일 I/O 없는 검증 유지, CLI의 불필요한 파일 초기화 제거. 부하 측정은 미실행 |
| 테스트 용이성 | 4 | DB 없는 실제 handler Router 검증과 기존 격리 CLI 프로세스 검증으로 발견 결함을 확인 |

복구 가능성은 기존 코드 재배포로 실행 동작을 되돌릴 수 있는 양방향이다. 스키마·업무 데이터·설정 형식은 바뀌지 않는다. 선택적 인증 추출 계약과 확인용 공개 경로 추가를 아키텍처 계약 변경으로 보아 R2를 유지한다. 새 me 호출자는 코드 롤백 시 경로가 없어지는 영향을 확인해야 한다. 이번에는 실제 배포·운영 롤백·부하·전체 workspace 및 DB auth 회귀를 실행하지 않았다. 이 판정은 검토한 변경의 리뷰이며 배포 승인은 아니다. (확인: 변경·검증 범위 / 추론: 복구·리스크)

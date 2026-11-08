---
id: PLN-AUTH-JWT-API
status: implemented
---

# JWT API 계약과 검증

앱이 시작할 때 `auth.jwt`의 Ed25519 키 쌍을 읽고 검증해 발급기와 공개키 검증기를 준비한다. `private_key`, `public_key`, `issuer`, `audience`가 필요하며 `generate_if_missing`은 기본 false다. 누락·불일치·잘못된 키나 빈 issuer/audience는 시작 실패다. 경로는 작업 디렉터리 기준이다. 검증기는 개인키·DB를 소유하지 않는다. (확인)

| 메서드·경로 | JSON 요청 | 성공 응답 |
| --- | --- | --- |
| POST `/api/v1/auth/local` | `email`, `password` | 200: `account_id`, `status`, `access_token`, `refresh_token` |
| POST `/api/v1/auth/refresh` | `refresh_token` | 200: `access_token`만 반환, 기존 Refresh 유지 |
| POST `/api/v1/auth/logout` | `refresh_token` | 200: `{}` |

토큰 응답과 오류는 `Cache-Control: no-store`다. Refresh 미존재·갱신 불가 시 401 `INVALID_REFRESH_TOKEN`; 기존 만료·폐기 행의 반복 로그아웃은 성공한다. 잘못된 JSON은 공통 오류 계약을 따른다. 토큰 원문은 로그에 기록하지 않는다. (확인)

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

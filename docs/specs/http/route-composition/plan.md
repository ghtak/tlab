---
id: PLN-HTTP-ROUTE-COMPOSITION
status: accepted
---

# API·웹 라우터 조립 책임 정리

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| follows | [앱 개발 구조](../../../architecture/app-structure.md) | feature의 작업 소유권과 앱의 조립 책임 |
| follows | [PLN-AUTH-JWT-WEB](../../auth/jwt-web/plan.md) | 쿠키·CSRF·자동 갱신의 기존 계약 |
| 관련 설계 | [PLN-AUTH-WEB-UI-SESSION](../../auth/web-ui-session/plan.md) | React의 웹 API 호출과 세션 관리 |

## 제안 요지

`http`가 API·웹의 노출 경로와 feature 조합을 소유하고, `auth`는 인증 정책과 인증 시나리오를 제공한다. `http/api.rs`와 `http/web.rs`를 같은 수준의 조립 모듈로 두고, 두 경로에서 공유하는 업무 라우터 목록은 `http`에 한 번 정의한다. 인증·응답 계약을 유지하면서 구조의 의도를 드러내는 수정안이다. 최초 구조 변경 후 사용자 요청에 따라 웹 URL prefix를 `/web/v1`으로 지정했다. 사용자가 이 문서에 따른 구현을 승인했으며, 아래 배치로 구현했다. (제안·추론)

## 변경 전 구조와 문제

[http.rs](../../../../tlab-app/src/http.rs)는 auth API를 merge하고 RBAC를 `/api/v1`에 nest한다. 반면 [auth/web.rs](../../../../tlab-app/src/auth/web.rs)는 당시 `/web` 전체를 만들면서 RBAC 및 사용자 생성까지 등록하고 CookieManager·guard·인증 middleware를 적용한다. [auth/route.rs](../../../../tlab-app/src/auth/route.rs)는 `/api/v1` 절대 prefix까지 알고 있다. (확인)

따라서 API 조합은 http에, 웹 조합은 auth에 흩어져 있다. 웹에서 제공하는 업무 기능을 찾으려면 인증 feature 내부를 읽어야 하고, 새로운 업무 feature를 양쪽에 추가할 때 두 위치를 수정해야 한다. 인증 feature가 다른 업무 feature의 웹 노출을 선택하는 책임은 앱 조립 쪽으로 옮기는 것이 적절하다. (추론)

## 권장 배치

```text
tlab-app/src/
├── http.rs                  # 최상위 경로·인프라·서버 실행과 종료
├── http/
│   ├── api.rs               # /api/v1 아래에 둘 라우터 조립
│   ├── web.rs               # /web/v1 아래에 둘 라우터와 보호 범위 조립
│   └── tests.rs             # 최종 앱 Router의 조립 계약 검증
├── auth/
│   ├── route.rs             # API/웹 인증 진입점의 상대 경로 등록
│   ├── web.rs               # 쿠키 정책·guard·자동 갱신·인증 marker
│   └── slice/...            # 로그인·갱신·종료·계정 생성의 handler/usecase
└── rbac/
    └── route.rs             # 양쪽에서 공유할 상대 경로 업무 라우터
```

API·웹은 HTTP 전송 방식과 정책의 구분이므로 `http` 아래의 형제 모듈로 둔다. 앱 루트에 별도 `api/`, `web/` 계층을 만들면 feature 구조와 경쟁하는 분류가 늘어난다. 이번 규모에서는 파일 두 개로 조립을 분리하는 것으로 충분하다. `auth/web.rs` 이름은 인증 feature의 웹 지원이라는 의미로 유지하되, 다른 feature 등록과 앱 전역 prefix는 제거한다. (제안·추론)

| 주체 | 소유 책임 | 제공 계약 |
| --- | --- | --- |
| `http` | 외부 prefix·전역 tracing·정적/운영 경로·서버 생명주기·공유 노출 목록 | 최종 앱 Router, 내부 `shared_routes()` |
| `http/api` | API 인증 진입점과 공유 업무 라우터 조합 | prefix 없는 API Router |
| `http/web` | 공개/보호 경로 선택·웹 middleware 적용 범위와 순서 | prefix 없는 웹 Router |
| `auth/route` | auth 소유 API/웹 엔드포인트와 handler 연결 | `api_routes()`, `web_public_routes()`, `web_protected_routes()` |
| `auth/web` | 쿠키·CSRF·캐시·갱신·검증 marker와 속성 | 보호 그룹 및 전체 웹 그룹에 적용할 인증 전용 조립 함수 |
| 업무 feature | 상대 경로·입출력·업무 권한 판단 | 공유 가능한 `router()` |

함수명은 의도를 설명하기 위한 제안이며 실제 Rust 공개 범위는 호출자에게 필요한 만큼만 `pub(crate)`로 연다. 인증 marker는 현재처럼 외부에서 생성하지 못하게 유지한다. auth slice의 handler를 앱 조립에서 직접 나열하기보다 auth 라우터 제공 함수를 사용한다. (제안)

## 조립 형태

아래 코드는 책임과 합성 순서를 나타내는 의사 코드다. 실제 구현은 같은 구조를 사용하며 내부 조립 함수의 공개 범위는 `pub(super)`, auth 제공 함수는 `pub(crate)`로 제한한다. (확인)

```rust
// http.rs
fn shared_routes() -> Router<Arc<Container>> {
    // 두 인증 방식에서 동일하게 제공하기로 선택한 업무 기능.
    Router::new().merge(crate::rbac::router())
}

fn router(container: Arc<Container>) -> Result<Router> {
    let routes = Router::new()
        .nest("/api/v1", api::router())
        .nest("/web/v1", web::router(container.clone()))
        // health, db_status, static files도 여기에서 조립
        ;
    Ok(trace_requests(routes).with_state(container))
}

// http/api.rs
fn router() -> Router<Arc<Container>> {
    Router::new()
        .merge(auth::api_routes())
        .merge(super::shared_routes())
}

// http/web.rs
fn router(container: Arc<Container>) -> Router<Arc<Container>> {
    let protected = Router::new()
        .merge(auth::web_protected_routes())
        .merge(super::shared_routes());
    let protected = auth::web::with_authentication(protected, container.clone());
    let web = Router::new()
        .merge(auth::web_public_routes())
        .merge(protected);
    auth::web::with_web_transport(web, container)
}
```

`shared_routes()`의 소유 위치는 `http.rs`의 작은 내부 함수로 시작한다. 각 전송에 필요한 Router를 새로 만드는 단순 조립 함수이며 동적 등록 프레임워크나 feature별 노출 flag를 도입하지 않는다. 현재는 RBAC 하나이지만 양쪽 노출 목록을 한 번 관리한다는 목적이 함수명과 호출에서 드러난다. 어느 한쪽에만 필요한 업무 기능은 해당 api/web 조립에서 명시적으로 추가한다. (제안)

웹 조립의 실행 순서는 바깥에서 `CookieManager → guard(Origin·쿠키 헤더·no-store) → 보호 경로의 authenticate → handler`다. `with_authentication`은 전달된 보호 Router에만 인증 route layer를, `with_web_transport`는 전달된 전체 웹 Router에 guard와 CookieManager를 적용한다. 두 함수는 feature 선택이나 URL 등록을 하지 않는다. Axum layer는 나중에 추가한 것이 바깥에 놓이므로 이 순서를 구현·테스트로 고정한다. (확인·제안)

API의 Bearer 검증은 기존처럼 필요한 handler의 `AccessClaims` extractor가 수행한다. 이번 구조 변경을 계기로 API 전체에 필수 인증 middleware를 새로 적용하지 않는다. 업무 권한 판단도 해당 handler/usecase에 남긴다. (확인·제안)

## auth가 제공할 경로 그룹

모든 경로는 외부 prefix를 제외한 상대 경로로 정의한다. `/api/v1`과 `/web/v1`은 `http`가 한 번만 지정한다. (제안)

| 그룹 | 상대 경로 | 기존 계약 |
| --- | --- | --- |
| API auth | `/auth/local`, `/auth/refresh`, `/auth/logout`, `/auth/me`, `/accounts/local` | JSON 토큰·Bearer 및 현재 계정 생성 계약 |
| 웹 공개 auth | `/auth/local`, `/auth/logout` | guard 적용, 자동 갱신 인증은 적용하지 않음 |
| 웹 보호 auth | `/auth/me`, `/accounts/local` | 쿠키 인증·자동 갱신 후 handler 실행 |
| 공통 업무 | `/rbac/...`, `/accounts/{account_id}/...` | 동일 업무 handler·권한 판단 |

사용자 생성은 현재 auth 소유 slice이므로 기존 위치를 유지한다. API와 웹에서의 적용 정책 차이도 보존한다. API `/auth/me`는 `sid`, `sub`를 반환하고 웹은 추가로 `user`를 반환하므로 두 handler를 무리하게 합치지 않는다. 로그인·로그아웃도 전송 계약이 달라 각각 유지한다. 공유 대상은 계약이 같은 업무 Router다. (확인·제안)

## 변경 범위와 검증

1. `auth/route.rs`의 prefix를 상대 경로로 바꾸고 웹 auth 경로 등록과 웹 me handler를 모은다. 기존 테스트가 전역 URL을 확인할 때는 http 조립 또는 외부 prefix를 붙인 테스트 Router를 사용한다.
2. `auth/web.rs`에서 앱 웹 Router 조립과 RBAC 의존을 제거하고, 인증 및 웹 전송 적용 함수만 제공한다. Config·쿠키·검증 marker·갱신 로직과 실패 계약은 유지한다.
3. `http/api.rs`, `http/web.rs`와 `shared_routes()`를 추가하고 최상위 http에서 두 prefix를 나란히 등록한다. auth 모듈의 재노출과 기존 테스트 호출부를 필요한 만큼 수정한다.
4. 최종 앱 Router에서 양쪽 RBAC 경로가 같은 권한 판단·업무 결과를 내는지 확인한다. API는 쿠키만으로 인증되지 않고 웹은 Bearer만으로 인증되지 않는지 확인한다.
5. 웹 공개/보호 경계, 로그인·로그아웃, 자동 갱신과 실패 시 쿠키 처리, CSRF 및 캐시 정책의 기존 테스트를 재사용한다. 기존 ignored DB 테스트는 실제 격리 DB 환경에서 실행해야 한다. fmt·컴파일·관련 앱 테스트를 수행하고 변경 영향에 따라 검증 범위를 정한다.

핵심 위험은 layer 이동에 따른 적용 범위·순서 변경과 경로 prefix의 누락/중복이다. URL·토큰·쿠키·DB 스키마 변경 없이 조립 책임만 이동시키는 범위로 제한하고, 변경된 최종 Router를 통해 경계를 검증한다. (추론·제안)

## 결정 상태와 남은 사항

- 사용자 선호: 라우터 단위 공유를 유지하고 조립 의도가 구조에 드러나게 한다. (확인)
- 권장안: `http/api.rs`와 `http/web.rs`를 형제로 두고 공유 업무 라우터 목록을 http가 소유한다. 인증 구현은 auth에 남긴다. 사용자 승인에 따라 구현했다. (확인)
- UI 연결에 필요한 Vite proxy와 origin 설정은 별도 UI 세션 설계 범위다. 이 구조 정리에서 개발 주소나 배포 방식을 추가 확정하지 않는다. (제안)

## 구현·변경 기록

- 현재 소스의 조립 위치와 인증 그룹을 대조하여 수정안 작성. 소스 코드 수정과 런타임 테스트는 수행하지 않았다. (확인)

### 라우터 조립 책임 분리 구현 (2026-11-11)

- `http.rs`가 `/api/v1`·`/web/v1` prefix와 공통 업무 목록 `shared_routes()`를 소유한다. `http/api.rs`·`http/web.rs`는 형제 모듈로 auth 그룹과 공유 RBAC Router를 조립한다. (확인)
- `auth/route.rs`는 prefix 없는 `api_routes()`·`web_public_routes()`·`web_protected_routes()`와 API/웹 me handler를 제공한다. API me의 `{sid, sub}`와 웹 me의 `{sid, sub, user}` 응답을 유지했다. slice handler의 공개 범위를 늘리지 않았다. (확인)
- `auth/web.rs`는 `with_authentication()`과 `with_web_transport()`를 제공하며 RBAC 등록·앱 경로 조립을 제거했다. CookieManager → guard → 보호 경로 authenticate → handler 순서와 비공개 marker 생성 경계를 유지했다. 쿠키·CSRF·갱신 구현과 업무 권한 판단은 변경하지 않았다. API에 전역 필수 인증 layer를 추가하지 않았다. (확인)
- auth slice 테스트는 상대 Router에 외부 prefix를 붙인다. 기존 웹 18개 테스트는 실제 `http::router`를 사용하도록 연결하고, 인증원 분리 테스트에서 실제 API me 및 응답 차이를 확인한다. 기존 RBAC 최종 Router 테스트에 양쪽 조회 결과와 무권한 사용자의 403 응답 동등성을 보완했다. 별도 테스트 전용 앱 조립이나 중복된 자원 준비를 추가하지 않았다. (확인)

자체 점검: 모든 auth 소유 경로의 상대 경로 전환과 최상위 prefix 등록을 대조했다. `auth/web.rs`에 앱 URL·RBAC 의존이 없고 marker의 tuple 필드는 계속 비공개다. 보호 그룹에만 `route_layer`를 적용하고 공개 그룹을 merge한 뒤 guard·CookieManager를 적용한다. 스키마·설정·의존성·잠금 파일·UI는 변경하지 않았다. 시작 시 있던 UI 설계 문서는 보존했다. (확인)

| 최종 검증 명령 | 결과 |
| --- | --- |
| `env TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --locked -- --include-ignored --test-threads=1` | 최종 코드: 앱 82 + bootstrap CLI 1 + RBAC storage 26 = 109 passed, 0 failed, 0 ignored. 실제 PostgreSQL 임시 스키마 사용 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 성공. 전체 target 컴파일 포함 |
| `cargo fmt --all -- --check` | 성공 |
| `git diff --check` | 성공 |

실제 브라우저·운영 배포·부하 검증은 미실행이다. 이번 변경은 URL과 인증 정책을 유지하는 조립 구조 변경이며 커밋·push는 수행하지 않았다. (확인)

### 웹 API prefix 버전 지정 (2026-11-11)

사용자 후속 요청에 따라 웹 API prefix를 `/web`에서 `/web/v1`으로 변경했다. 최상위 `http.rs`의 nest와 실제 Router 테스트, REST client 예제, README 및 관련 인증·RBAC·UI 설계의 현재 경로를 갱신했다. 인증·쿠키·CSRF·자동 갱신 정책은 유지하며 기존 `/web/auth/*` 경로는 제공하지 않는다. 앞선 109개 통과 기록은 prefix 변경 전 검증이며 이번 변경 검증은 아래에 별도로 기록한다. (확인)

최종 검증: 실제 로컬 PostgreSQL의 격리 스키마를 사용한 `cargo test -p tlab-app --locked auth::web::tests -- --include-ignored --test-threads=1`은 18 passed, `cargo test -p tlab-app --locked production_web_rbac_cookie_csrf_and_automatic_refresh -- --include-ignored --test-threads=1`은 1 passed다. 두 명령 모두 `TLAB_TEST_DATABASE_URL`을 지정했으며 실패·ignored는 0이다. 새 prefix에서 로그인·me·로그아웃·CSRF·갱신·RBAC를 확인하고 이전 `/web/auth/me`는 404임을 검증했다. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`도 통과했다. 이번 prefix 변경 후 앱 전체 테스트는 재실행하지 않았으며 실제 브라우저 검증도 미실행이다. (확인)

### 코드 리뷰 (2026-11-11)

- 대상: `dcace4b` 대비 커밋하지 않은 라우터 조립·웹 prefix 변경. 필수 검사 `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` 통과. (확인)
- `cargo audit` 종료 코드 1: 잠금 파일의 `libsqlite3-sys 0.37.0`에 [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459) 1건, 수정 버전 `>=0.39.0`. 기본 대상의 `cargo tree -i libsqlite3-sys --locked`에는 활성 의존성이 나타나지 않았다. 다른 대상·feature의 영향은 미확인이다. audit은 리뷰를 차단하지 않는다. (확인)
- 변경 코드에서 별도의 결함은 발견하지 못했다. 웹 URL은 호환되지 않으므로 소비자 전환과 롤백 시 경로 복원을 확인해야 한다. 실제 배포·브라우저·롤백 검증 및 외부 소비자 현황은 미확인으로 판정을 보류한다. (추론)
- 품질 점수: 기능 3, 가독성과 구조 4, 안정성 3, 보안 3, 성능 4, 테스트 용이성 3 (각 4점 만점, 총 83.75/100). 경로 조립과 인증 경계 테스트는 있으나 실제 브라우저·전환 검증은 남아 있다. 의존성 advisory의 다른 대상·feature 영향도 확인이 필요하다. (추론)
- 리스크 미확정: HTTP 조립 책임과 웹 URL 계약이 바뀌어 아키텍처 변경이 있다. DB 스키마 변경은 없으나 기존·신규 클라이언트와 라우팅 설정의 배포 및 롤백 순서는 확인되지 않아 PR 롤백만으로 복구 가능한지 미확인이다. (추론)
- 추가 검증 `cargo test --workspace --locked` 성공. PostgreSQL 의존 테스트 등 ignored 항목은 이번 명령에서 실행되지 않았고 위의 기존 대상별 DB 검증 기록을 참고한다. (확인)

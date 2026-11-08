---
id: VER-DOCUMENT-CODE-CONSISTENCY
status: draft
---

# 문서·코드 일치 조사

최초 조사 결과에 대한 사용자 후속 요청을 반영했다. 현재 반영 상태와 실행 검증은 마지막 `후속 반영 결과` 절을 따른다. 아래 발견 사항의 문서·행 번호는 최초 조사 기준 커밋의 위치이며, 수정 전 근거와 제안 경위를 보존한다. (확인)

## 조사 기준과 범위

- 요청: herdr subagent로 범위를 나누어 문서·코드 불일치를 검증하고 반영할 내용을 전달받는다.
- 기준: 2026-11-08, Git `9ce52646fcb02709cbe7558dd7ba9bd2465865d5`. 조사 시작 시 작업 트리는 깨끗했다. (확인)
- 방법: 문서의 현재 계약·계획·과거 검증 기록을 구분하고 소스·설정·테스트 내용과 정적으로 대조했다. 이번 조사에서 앱·DB·브라우저 실행이나 테스트 재실행은 하지 않았다. 기존 문서의 통과 기록을 이번 실행 결과로 취급하지 않는다.
- 제외: `docs/notes`, 외부 서비스·운영 배포 상태, 과거 대화의 실제 컨텍스트 사용량·잘림 여부.
- 이 문서는 조사 결과와 반영 제안이다. 기존 명세·실행 코드를 수정하거나 새 동작 정책을 채택한 문서가 아니다.

## 분할과 전달 계약

herdr-core의 `hc run --bg --parent t2c`로 독립적인 대화의 Codex를 실행했다. 각 agent는 `--no-daemon`으로 실행했고 추가 agent 생성과 원본 수정을 금지했다. 담당 범위, 근거 위치, 실제 동작, 영향, 문서/코드 반영 방향, 구체 조치를 보고하도록 지정했다. (확인)

| 담당 | herdr terminal | 범위 |
| --- | --- | --- |
| 인증·JWT | `t2h` / `audit-auth` | `docs/specs/auth`, 인증 slice·정책·repository·migration |
| HTTP·앱·UI | `t2j` / `audit-http` | `docs/specs/http`, 앱 구성·실행·응답, HTTP 라이브러리, UI·README |
| 공통 기능 | `t2k` / `audit-core` | DB·migration·설정·키파일·비밀번호·오류·tracing, 검증 도구·just |
| 부모 | `t2c` | 문서 진입점·링크·상태 규칙, 결과 교차 대조·통합 |

## 후속 수정 범위와 완료 조건

사용자가 이 보고서의 AUTH-01~06·HTTP 보완안, CORE-02~05·DOC-01~02 반영을 요청했다. CORE-01은 제시된 대안 중 URL을 숨기는 커스텀 Debug 구현을 명시적으로 선택했다. 보고서의 구체 반영안을 기준으로 요구사항·설계가 정해졌으며 그 범위에서 구현·검증·자체 점검을 수행한다. (확인)

- CORE-01 요구사항: 일반·대체 형식 Debug에서 DB URL 전체를 숨기고 연결 수는 표시한다. 상위 앱 Config의 중첩 Debug에서도 URL은 숨겨야 한다. 설정 로딩·Clone·DB 접속 계약은 유지한다.
- CORE-01 설계: DB Config의 자동 Debug를 수동 구현으로 교체하고 `url`은 고정 `[REDACTED]`로 출력한다. DB 설정과 앱 설정의 Debug를 직접 실행하는 DB 없는 회귀 테스트로 검증한다. 사용자 요청 “debug 커스텀을 해서 표시안되게 수정”이 이 설계 선택의 근거다.
- 문서 수정: 현재 계약·단계 시점·경로·실행 사전 조건을 원본과 맞추고 과거 검증·리뷰는 당시 기록으로 보존한다. salt는 공개 API를 유지하는 안으로 문서화한다. 문서 상태는 구현 완료만으로 올리지 않고 채택 근거를 기록한다.
- 완료 조건: 요청한 문서·주석 수정, URL 비노출 회귀 테스트·관련 기본 테스트와 fmt·Clippy 통과, 최종 diff·링크·문서 상태 대조. 이번 작업은 DB 데이터·schema 동작이나 실제 앱 인증 동작을 바꾸지 않는다.

## 우선 반영할 내용

| 우선순위 | 항목 | 필요한 반영 |
| --- | --- | --- |
| 높음 | CORE-01: DB URL 비밀 보호 계약 | DB 설정의 자동 Debug 제거 또는 URL을 숨기는 Debug 구현. 상위 앱 설정도 함께 확인 |
| 높음 | AUTH-01·02: Refresh 잠금·트랜잭션 | 저장 문서와 repository·SQL 주석을 현재 갱신·로그아웃 계약으로 정리 |
| 높음 | AUTH-03: 로그인 HTTP 계약 | 내부 `execute`의 Account 반환과 HTTP의 토큰 발급·Refresh 저장을 구분 |
| 높음 | CORE-02: 앱 실행 사전 조건 | DB 연결·JWT 키·두 서버 TLS 준비를 HTTP 검증 도구와 Feature map에 반영 |
| 보통 | HTTP-02·AUTH-04 | 실행·종료 담당을 `http::run`으로 수정하고 토큰 로그 비기록의 적용 범위 명시 |
| 보통 | 나머지 본문·경로·상태 | 낡은 현황·배치·공개 API·깨진 링크를 보완하고 문서 채택 상태 정리 |

위 우선순위와 반영 방향은 조사에 따른 제안이다. DB Debug는 코드 수정 후보이며 실제 로그 유출을 관찰한 결과가 아니다. 다른 항목 대부분은 현재 합의·구현을 문서와 주석에 전달하는 변경이다. (추론)

## 컨텍스트 관련 판단

Refresh 저장 문서의 현재 책임 절과 최신 lifecycle 문서가 서로 다른 잠금 계약을 설명한다. 로그인 명세의 기본 인증 계약과 현재 HTTP 토큰 응답도 구분해서 읽어야 한다. 구현·리뷰 이력을 유지하는 것과 별개로, 본문에서 현재 적용 범위와 후속 문서 연결이 부족한 곳이 있다. (확인)

최초 조사 상태에서는 읽은 문서에 따라 과거 계약을 복원하거나 이미 구현한 기능을 다시 구현할 위험이 있었다. 후속 반영에서는 현재 계약을 소유하는 문서의 본문을 갱신하고 과거 기록은 대상 시점을 명시해 보존했다. 실제 원인이 모델의 컨텍스트 한도·잘림이었다고 단정할 근거는 없다. (확인·추론)

## 인증·JWT 보고와 반영안

### AUTH-01: Refresh 저장 문서의 포괄적인 잠금 요구

- 문서: [저장 설계](../auth/jwt-refresh-storage/plan.md) 14행은 모든 Refresh 시작 경로에 해시 조회 → 계정 잠금 → Refresh 잠금·재조회를 요구한다. [repository](../../../tlab-app/src/auth/repository/refresh_token.rs) 25행의 `find_by_hash` 주석에도 같은 전제가 남아 있다. (확인)
- 실제 코드: [갱신](../../../tlab-app/src/auth/slice/refresh_access_token/usecase.rs) 29~61행은 일반 연결에서 두 SELECT 후 Access를 서명한다. [로그아웃](../../../tlab-app/src/auth/slice/logout/usecase.rs) 22~41행은 트랜잭션에서 해시 조회·조건부 폐기·commit을 수행하며 선행 계정·Refresh 조회 잠금은 없다. UPDATE 자체의 행 잠금은 발생한다. (확인)
- 기준 대조: [lifecycle 현행 계약](../auth/jwt-refresh-lifecycle/plan.md) 10~20행과 [JWT 정책](../auth/jwt.md) 102행은 현재 구현과 일치하며 경합 완화의 합의도 기록한다. 저장 문서대로 잠금을 재도입할 근거가 아니다. (확인·추론)
- 반영안: “트랜잭션과 잠금은 호출 시나리오가 소유한다. 로그인과 계정 상태 변경·전체 폐기는 같은 트랜잭션에서 계정 행을 먼저 잠근다. 갱신은 일반 연결에서 잠금 없는 조회를 사용한다. 일반 로그아웃은 선행 조회 잠금 없이 조건부 UPDATE 후 commit한다. `lock_by_hash`를 사용하는 경로만 같은 트랜잭션의 계정 잠금 이후 Refresh를 잠그고 재조회한다.” 저장 설계의 잠금 후 시각 결정도 로그인 발급에 한정하고 lifecycle 계약을 연결한다. `find_by_hash` 주석은 필터 없는 조회이며 정책은 호출자 책임임을 표현한다. **문서·코드 주석 수정 제안**이다.

### AUTH-02: migration 주석의 갱신 트랜잭션 설명

- 문서 역할의 원본: [0002 SQL](../../../tlab-app/migrations/0002_create_tlab_auth_refresh_token.sql) 10행은 로그인·갱신·로그아웃·상태 변경을 각각 트랜잭션으로 처리한다고 한다. 현재 갱신에는 명시적 tx·commit이 없다. 12~14행의 잠금·경합 설명은 최신이지만 10행은 이전 계약이다. (확인)
- 반영안: “로그인·로그아웃과 계정 상태 변경·전체 폐기 조합은 호출자가 트랜잭션을 소유한다. 갱신은 일반 연결에서 독립 SELECT 두 번을 수행하며 명시적 트랜잭션·commit과 동일 시점 스냅샷을 보장하지 않는다.” **SQL 주석 수정 제안**이며 schema 변경은 필요하지 않다.

### AUTH-03: 로그인 내부 API와 HTTP 토큰 발급 계약 혼재

- 문서: [로컬 로그인 설계](../auth/login-local/plan.md) 20·104·148~151·183행에는 토큰·세션 미생성, ID·상태만 응답, DB 변경 없음이 남아 있다. [로그인 발급](../auth/jwt-login/plan.md) 10행도 계정만 반환하는 “HTTP 계약 유지”라고 한다. [계정 산출물 안내](../auth/account.md) 15행은 같은 이전 HTTP 설명이다. (확인)
- 실제 코드: [로그인 route](../../../tlab-app/src/auth/slice/login_local/route.rs) 15~40행은 `execute_with_tokens`를 호출해 `account_id`, `status`, `access_token`, `refresh_token`을 응답한다. [usecase](../../../tlab-app/src/auth/slice/login_local/usecase.rs) 69~115행은 인증 후 계정 잠금·Refresh INSERT·서명·commit을 수행한다. 내부 `execute -> Account`는 유지된다. [JWT API](../auth/jwt-api/plan.md) 12행은 최신 HTTP와 일치한다. (확인)
- 반영안: “`LocalLogin::execute`는 자격증명·계정 상태를 확인해 Account를 반환한다. 현재 `POST /api/v1/auth/local`은 `execute_with_tokens`를 호출하고 Refresh 저장·Access 서명·commit 후 계정 ID·상태와 Access·Refresh를 반환한다. HTTP·토큰 정책은 JWT API와 로그인 발급 설계를 따른다. 쿠키는 설정하지 않는다.” 검증표의 “DB 변경 없음”은 내부 `execute` 직접 호출로 한정한다. jwt-login의 “HTTP 계약 유지”는 “내부 Account 반환 계약 유지”로 바꾼다. **문서 수정 제안**이며 과거 리뷰의 쓰기 없음·롤백 평가는 당시 기록으로 보존한다.

### AUTH-04: 토큰 로그 비기록의 범위가 본문과 후속 결정에서 다름

- 문서: [JWT API](../auth/jwt-api/plan.md) 16행은 토큰 원문을 로그에 기록하지 않는다고 단정하지만 57행에는 query 전달 토큰의 노출 방지를 범위 밖으로 두고 전체 URI 기록을 유지한 사용자 결정이 있다. (확인)
- 실제 코드: [요청 span](../../../tlab/src/http/trace.rs) 35행은 URI 전체를 기록한다. 정상 전달은 Bearer 헤더와 JSON 본문이며 query를 인증 근거로 사용하지 않는다. 실제 로그 유출을 관찰한 것은 아니다. (확인)
- 반영안: “정상 전달 계약인 Authorization 헤더의 Access와 JSON 본문의 Refresh를 별도 로그 필드로 기록하지 않는다. 공통 요청 로그는 전체 URI를 기록하며 query에 잘못 전달한 토큰의 로그 노출 방지는 기존 사용자 결정에 따라 이번 범위에서 제외한다.” **문서 수정 제안**이다. 이번 조사에서 URI 마스킹 정책을 새로 도입하지 않는다.

### AUTH-05·06: 단계 시점과 slice 경로 보완

- [토큰 기반 설계](../auth/jwt-token/plan.md) 25~27·74행의 호출자·DB·HTTP 미연결은 최초 토큰 기반 단계의 범위다. 현재 [Container](../../../tlab-app/src/app.rs) 137~158행과 auth route는 이미 연결되어 있다. 과거 단계 설명 자체를 기능 버그로 세지 않는다. (확인·추론)
- 반영안: 해당 본문에 “토큰 기반 최초 구현 단계에서는”을 붙이고, 현재 연결은 storage·login·lifecycle·API 문서를 따른다고 링크한다. 웹 쿠키·CSRF와 공개키 운영은 계속 후속으로 구분한다. `auth/token.rs:3~5`, `auth/repository/refresh_token.rs:2~4`, `auth/mod.rs:13`, `slice/login_local/usecase.rs:29,62`의 `later`·`API step 6` allow 사유도 현재 사용 관계로 정리한다. 불필요한 allow를 제거한다면 적절한 컴파일·Clippy 확인이 필요하다.
- [사용자 추가](../auth/create-local-user/plan.md) 32~44행과 [로컬 로그인](../auth/login-local/plan.md) 43~53행의 본문 구조도는 `auth/create_local_user`, `auth/login_local`로 안내하지만 실제는 `auth/slice/...`다. **도식에 `slice/`를 추가하는 문서 수정 제안**이다. 당시 코드 리뷰의 경로·해시는 보존한다. (확인)

## HTTP·앱·UI 보고와 반영안

### HTTP-01·02: 깨진 링크와 실행·종료 책임 오기

- [앱 HTTP 명세](../http/app/spec.md) 185행의 `../../account/create-local-user/plan.md#요청-tracing에-따른-오류-로그-책임-정리`는 대상 파일이 없다. 실제 [auth 변경 기록](../auth/create-local-user/plan.md) 293행에 해당 제목이 있다. 경로를 `../../auth/create-local-user/plan.md#요청-tracing에-따른-오류-로그-책임-정리`로 바꾼다. 과거 검증 결과 문구는 유지한다. (확인·제안)
- 같은 명세 191행은 main이 try_join·종료 요청을 수행한다고 한다. 실제 [main](../../../tlab-app/src/main.rs) 23행은 위임하며 [http::run](../../../tlab-app/src/http.rs) 12~35행이 Router·실행·신호·종료를 조정한다. 아키텍처 개요는 실제 코드와 일치한다. (확인)
- 대체 문구: “`http::run()`은 두 서버의 run과 OS 신호 대기를 try_join!으로 함께 기다린다. 신호를 받으면 각 Server::graceful_shutdown()을 호출하고 run 완료까지 기다린다. main은 설정·tracing 초기화와 실행 경로 선택을 담당한다. 실행·신호 오류 시 나머지 future를 취소하며 OS 신호는 http::shutdown_signal()에서 선택한다.” **문서 수정 제안**이다. 명시적 변경 이력의 main 설명은 당시 기록으로 보존한다.

### HTTP-03~05: 계약 위반과 구분할 보완

| 항목 | 근거·실제 상태 (확인) | 반영 제안 |
| --- | --- | --- |
| 정적 Router의 상태 제네릭 | `docs/specs/http/plan.md:56~58`은 상태 없는 반환 서명만 제시. `tlab/src/http.rs:68~70`은 `router<S>() -> Result<Router<S>> where S: Clone + Send + Sync + 'static` | 실제 제네릭·제약을 API 표에 반영. 상태 주입 전 merge가 가능하고 서버에는 최종 `Router<()>`를 전달한다고 명시 |
| `/db_status` 문서화 | `tlab-app/src/http.rs:17,49~50`은 인증 없는 GET에 HTTP 200과 `max_connections`, `size`, `idle` JSON 제공. `tlab/src/sqlxdb.rs:55~62`는 연결 획득·쿼리 없이 관측값을 읽음 | 현재 노출·응답을 문서화하고 운영 접근 정책은 별도 확인. 원자적 스냅샷·DB readiness 보장이 아님을 명시 |
| 소비자 예시와 현재 앱 구분 | `docs/specs/http/spec.md:52~76`, `plan.md:78`의 Option·하나 이상 활성화는 라이브러리 소비자 예시. 현재 `tlab-app/src/app.rs:73~90`은 HTTP·HTTPS 모두 필수 | 예시 뒤에 현재 앱은 두 블록 필수이며 앱 명세를 따른다고 연결 |

`/db_status`는 기존 문서가 전체 라우트 목록을 보장하지 않으므로 계약 위반으로 세지 않았다. 인증 추가·경로 제거를 이번 조사에서 결정하지 않는다. UI 실행·라우팅·스타일 설정과 두 README에서 변경이 필요한 명백한 불일치는 찾지 못했다. (확인·추론)

## 공통 기능 보고와 반영안

### CORE-01: DB Config의 비밀 보호 계약과 자동 Debug 충돌

- 문서: [DB 설계](../database/plan.md) 110행은 URL 자격 증명 때문에 Config에 자동 Debug를 제공하지 않는다고 한다. [sqlxdb 소스](../../../tlab/src/sqlxdb.rs) 13행 주석도 Debug 미제공을 약속한다. (확인)
- 실제 코드: 같은 파일 14~16행은 `#[derive(serde::Deserialize, Debug, Clone)]`와 `pub url: String`이다. [상위 앱 Config](../../../tlab-app/src/app.rs) 95~101행도 Debug를 파생하고 DB 설정을 포함한다. 파생 Debug는 문자열 URL 원문을 표시한다. (확인)
- 영향: DB 설정 또는 상위 앱 설정을 Debug 출력하면 URL의 자격 증명이 표시될 수 있다. 실제 운영 로그 유출은 미확인이다. (추론)
- **코드 수정 제안**: 계약 그대로 Debug를 제거하려면 상위 앱 Config도 조정해야 한다. Debug 사용을 유지할 필요가 있다면 수동 Debug에서 URL 전체를 `[REDACTED]`로 표시하고 연결 수만 실제 값으로 표시하는 안이 있다. 이 경우 문서·주석의 계약도 “URL을 숨긴 Debug”로 함께 바꾼다. DB 설정과 상위 앱 설정 어느 쪽의 Debug에도 비밀 문자열이 나타나지 않는 회귀 검증이 필요하다. 두 방향 중 선택은 후속 수정에서 확정한다.

### CORE-02: HTTP 검증 안내의 시작 사전 조건 누락

- 문서: [HTTP 도구](../../verification/tools/http/README.md) 12~16행은 DB 가동이 필요 없다고 안내한다. [Feature map](../../verification/feature/http-health.md) 6·11행도 DB 불필요·기본 설정 시작을 전제로 한다. 과거 실행 기록이 아닌 현재 사용 절차다. (확인)
- 실제 코드: [main](../../../tlab-app/src/main.rs) 23행과 [Container](../../../tlab-app/src/app.rs) 137·147행은 JWT 자료·DB 연결 준비를 마쳐야 HTTP 실행에 도달한다. [기본 설정](../../../tlab-app/config/app.toml) 20~29행의 JWT 생성 기본값은 false, DB URL은 빈 값이며 DB 생성자는 이를 거부한다. (확인)
- **문서 수정안**: “현재 앱 시작에는 접속 가능한 PostgreSQL URL과 유효한 JWT 개인키·공개키 설정이 필요하다. DB URL은 `.env` 또는 `TLAB_APP_DATABASE__URL`로 지정한다. 키 파일을 준비하거나 격리된 개발 환경에서 생성 옵션을 명시한다. HTTP·HTTPS 두 서버의 TLS 설정도 준비한다. `/health` 요청 자체는 DB readiness를 검사하지 않는다.” README의 최신 실행 안내를 연결한다.
- Feature map 15·19행의 선택 HTTPS·두 서버 모두 비활성화는 현재 앱의 두 설정 필수 정책에 맞춰 “HTTP·HTTPS 각각 확인”, “필수 설정 누락·TLS 조합 오류 확인”으로 보완한다. 라이브러리의 선택적 조립 계약은 별도 범위로 유지한다.

### CORE-03~05: 낡은 현재 상태와 salt API 설명

| 항목 | 문서 주장·실제 근거 (확인) | 반영안 |
| --- | --- | --- |
| 비밀번호 앱 사용처 | `password/hash/spec.md:25`, `plan.md:18`은 현재 앱 사용처가 없다고 함. `tlab-app/src/app.rs:100,141,152`에서 Argon2id 조립·소유, 가입·로그인 usecase가 hash·verify 호출 | 현재 앱의 소비자 존재를 명시하고 계정 정책은 소비 기능의 책임으로 연결. 초기 구현·리뷰의 사용처 없음은 당시 기록으로 유지 |
| 공용 salt 함수 | `password/hash/plan.md:38`은 salt 공통 함수가 없다고 함. `tlab/src/password.rs:20`의 공개 `salt()`를 `argon2id.rs:67`, `pbkdf2_sha256.rs:42`가 공유 | 유지한다면 공개 API와 OS 난수 16바이트·PHC 인코딩·Internal 실패 계약을 명세화. 내부 전용 의도라면 외부 소비자 영향 확인 후 가시성 축소. 공개 의도는 미확인 |
| 설정 모듈 현재 상태 | `config/load/spec.md:41`은 설정 모듈이 없다고 함. `tlab/src/lib.rs:1`의 공개 모듈과 `config.rs:54`의 load, 같은 문서 하단 구현 기록은 존재 | “설계 시작 당시 설정 모듈이 없었다. 현재는 tlab::config::load가 파일·환경변수 병합을 제공한다”로 시점 명시 |

salt 생성값·오류 동작의 결함을 확인한 것은 아니다. 공통 함수의 존재·공개 여부와 문서의 차이를 구분했다. (확인)

## 문서 관리에서 확인한 보완 사항

### DOC-01: 합의 상태와 구현 상태 혼용

- 근거: `docs/specs/auth/jwt-{api,login,refresh-storage,refresh-lifecycle}/plan.md:3`은 모두 `status: implemented`다. [문서 관리 규칙](../../agent/document-management/index.md)의 공통 메타데이터·상태 절은 `draft`, `accepted`, `retired`를 정의하고 구현 완료 여부와 채택 상태를 구분한다. (확인)
- 영향: 구현 완료를 합의·채택 상태로 잘못 해석하거나 상태 기반 탐색에서 빠뜨릴 수 있다. (추론)
- 반영 제안: 각 문서의 채택 근거를 확인한 후 허용된 상태로 정리한다. 근거 확인 없이 일괄 `accepted`로 바꾸지 않는다. 구현 여부는 본문의 구현 요약·검증 결과로 표현한다.

### DOC-02: 가이드의 근거 링크가 존재하지 않음

- 근거: [앱 수준 검증 가이드](../../agent/create-verification/index.md)의 20행 `[정리 메모](note.md)`는 실제 대상 파일이 없다. 문서 템플릿 안의 가상 링크는 발견 사항에 포함하지 않았다. (확인)
- 반영 제안: 실제 근거가 확인되면 해당 경로로 교체하고, 확인되지 않으면 이 링크를 제거한다. `docs/notes`를 임의로 탐색해 대체 근거를 만들지 않는다.

## 범위와 한계

소스에 구현된 계약을 대조한 조사이며 운영에서 동작을 재현한 결과가 아니다. 명시적으로 당시 범위·코드 해시·후속 보완이 기록된 과거 리뷰와 테스트 수치는 현재 불일치로 세지 않는다. 제안·미구현 상태로 표시된 SNS·탈퇴·웹 쿠키·CSRF·키 운영을 자동으로 구현 결함으로 분류하지 않는다.

| 영역 | 일치 확인한 주요 범위 (정적 대조) |
| --- | --- |
| 인증 | 가입 입력·원자 저장·오류, LOCAL 이메일 조회·dummy PHC·공유 permit, EdDSA·필수 클레임·600초, 로그인 저장·서명·commit 순서, 168시간·Refresh 반복 사용·폐기·현재 경합 정책, Bearer 추출·API 응답 |
| HTTP·앱·UI | Container/main/http 책임, 필수 두 서버·설정 진입점·migration 분기, 서버 new/run·종료·TLS 재로드·정적 파일, trace context·명시적 실패 로깅, UI 빌드·별도 실행·라우팅·스타일 설정 |
| 공통 | DB 연결·Context·트랜잭션·savepoint·상태 관측, migration 정렬·사전 검증·실패 처리, 설정 병합·오류, 키파일 조건부 생성·검증·게시·비밀 제거, 해싱 기본값·PHC·오류, 공통 오류, tracing 주석·테스트, DB 도구·just |

대조 대상 문서는 auth 11개, HTTP 5개, database 5개, config 1개, keyfiles 1개, password 2개, error 2개, verification Markdown 5개와 관련 아키텍처·README다. 백서는 방향 초안으로 구분했으며 예시를 현재 계약으로 전면 채택하지 않았다. 문서 관리 규칙·조사 가이드와 로컬 파일 링크도 확인했다. 외부 링크·모든 앵커의 유효성은 전수 검증하지 않았다. (확인)

agent 원보고서는 `/tmp/tlab-doc-audit-20261108/{auth,http-app,core}.md`에 받았고 부모가 세 파일 모두 읽어 핵심 근거를 재대조했다. 이 보고서에는 원본 파일 위치·반영안을 보존했다. 임시 보고서가 삭제되어도 위 근거로 후속 작업을 수행할 수 있다. (확인)

최초 조사에서는 실제 DB 제약 적용·운영 키·설정·로그 출력, TCP/HTTPS·브라우저·OS별 동작·성능·배포·롤백은 미확인이었다. 최초 조사 시점에는 테스트를 실행하지 않았고 저장소 변경은 이 보고서 추가뿐이었다. 이후 변경·실행은 다음 절에 별도로 기록한다.

보고서의 로컬 링크 대상·메타데이터·공백 검사는 통과했다. 세 agent의 완료 상태와 결과 수신을 확인한 후 이번 조사에서 시작한 terminal만 종료·제거했다. (확인)

## 후속 반영 결과

사용자는 AUTH-01~06, HTTP 보완안, CORE-01~05, DOC-01~02를 순차적으로 반영하도록 요청했다. CORE-01은 URL을 숨기는 커스텀 Debug, CORE-04는 기존 공개 salt API를 유지하면서 문서화하는 안으로 반영했다. (확인)

| 항목 | 반영 결과 |
| --- | --- |
| AUTH-01·02 | 저장 설계와 repository·SQL 주석을 경합 완화·일반 연결 갱신으로 정리. 반복·동시 갱신·재시도는 Refresh·세션의 최초 로그인 +168시간 만료를 연장하지 않으며 새 Access는 발급부터 600초임을 명시 |
| AUTH-03 | 내부 execute의 Account 반환·읽기 인증과 HTTP의 Refresh 저장·토큰 발급을 로컬 로그인·발급·계정 안내·검증 조건에서 구분 |
| AUTH-04 | 정상 헤더·본문의 별도 토큰 로깅 없음과 전체 URI의 query 노출 한계를 본문에 명시 |
| AUTH-05·06 | 토큰 기반 최초 단계와 현재 연결 문서를 구분하고 slice 경로 도식 수정. 과거 기록은 보존. 런타임과 무관한 기존 allow 사유 정리는 이번 문서 수정에 포함하지 않음 |
| HTTP-01~05 | auth 링크·http::run 책임·정적 Router 제네릭·소비자 예시와 앱의 두 서버 필수 정책 반영. /db_status의 실제 공개 상태·응답·관측 한계를 문서화하고 운영 접근 정책은 미확정으로 유지 |
| CORE-01 | sqlxdb::Config 수동 Debug로 URL 전체를 [REDACTED] 처리. 최대 연결 수는 유지. 직접·중첩 및 일반·대체 형식의 비밀 비노출 회귀 테스트 추가. DB 설계·주석 일치 |
| CORE-02 | HTTP 도구·Feature map에 DB URL·JWT 키·TLS 준비와 두 서버 필수 조건 명시. 요청 자체의 DB readiness 미검사와 구분 |
| CORE-03~05 | 해싱 앱 소비자·복구 영향, 공개 salt API·오류 계약과 설정 모듈 도입 전후 시점 반영 |
| DOC-01 | 기존 합의·구현 기록과 이번 사용자 반영 요청에 근거해 4개 문서의 채택 상태를 accepted로 정리. 각 문서에 근거를 기록하고 구현·검증 상태와 구분 |
| DOC-02 | 존재하지 않는 정리 메모 링크 문장 제거 |

### 실행 검증

- 변경 전 `cargo test -p tlab --test sqlxdb --locked --offline config_debug_hides_database_url`: 새 비노출 테스트가 기존 자동 Debug에서 실패했다. 실패 메시지에 URL·비밀값은 출력하지 않았다. (확인)
- 구현 후 `cargo test --workspace --locked --offline`: 통과. DB Config 직접·앱 Config 중첩 Debug 회귀 테스트 모두 통과했다. 일반 실행의 외부 DB ignored 테스트는 실행하지 않았다. HTTP·키파일 등 기본 실행과 doctest도 통과했다. (확인)
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: 통과. (확인)
- 변경 Markdown의 실제 로컬 링크와 specs 메타데이터 상태 규칙, `git diff --check`: 통과. 가상 예시·인라인 코드 안의 과거 링크는 검사 대상에서 구분했다. (확인)

### 자체 점검과 한계

DB URL 숨김은 Config의 Debug 경계가 책임지므로 상위 앱의 파생 Debug에도 적용된다. 설정 역직렬화·Clone·DB 생성 API는 유지한다. SQL은 주석만 변경했으며 DDL·인증 usecase·만료 판단·salt 생성 동작은 변경하지 않았다. 문서의 현재 계약과 과거 리뷰를 구분하고 수정 범위·링크·채택 근거를 대조했다. 정식 코드 리뷰 점수는 산정하지 않았다. (확인)

실제 운영 로그·외부 DB 연동·브라우저·운영 배포·롤백은 수행하지 않았다. /db_status 운영 접근 정책과 기존에 후속으로 명시한 웹·키 운영 등은 이번 요청에서 새로 확정하지 않았다. 커밋·배포는 수행하지 않았다. (확인)

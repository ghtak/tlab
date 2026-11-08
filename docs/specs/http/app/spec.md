---
id: SPEC-HTTP-APP
status: draft
---

# 앱 HTTP 연결과 생존 확인

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| follows | [PLN-HTTP](../plan.md) | 단일 서버 구현체와 상위 인스턴스 조립 계약 |

## 범위와 결정

사용자의 HTTP 앱 연계 요청과 후속 경로 확인에 따라 `tlab-app`에 `/health`를 추가한다.
앱이 설정 선택·Router 조립·다중 인스턴스 실행·종료를 맡고 `tlab::http`의 생성·실행 API를 사용한다. 후속 정리에 따라 `new/run` 호출로 이행한다. (확인)

| 항목 | 이번 선택과 이유 |
| --- | --- |
| 상태 확인 | `GET /health` → 200, `text/plain; charset=utf-8`, 본문 `ok`. 외부 DB·서비스를 검사하지 않는 생존 확인 |
| 기본 실행 | HTTP `127.0.0.1:8080`과 HTTPS `127.0.0.1:8081`을 함께 실행. 개발용 TLS 파일이 없으면 생성. 정적 파일은 설정할 때 연결 |
| 설정 | 앱의 `server.http`, `server.https`에 각각 `http::Config`, 공통 `server.static_files`에 `StaticFilesConfig` 포함 |
| 그룹 준비 | 활성 인스턴스의 TLS를 모두 준비한 다음 실행. 리스너 바인딩은 각 run에서 진행 |
| 그룹 실행 | 같은 Router를 전달하고 `try_join!` 사용. 한 실행의 실패는 다른 실행 future를 drop하고 앱 오류로 전달 |
| 종료 | 앱이 Ctrl+C, Unix에서는 SIGTERM도 받음. 공통 종료 신호를 각 인스턴스에 전달하고 완료를 기다림 |
| 검증 도구 | `just`는 명령 진입점, `curl`은 요청·응답 관찰. 다중 요청·상태·JSON 검증이 필요해질 때 Python `requests` 추가 |

상태 본문·기본 포트·종료 방식은 HTTP 최초 연결의 선택이다. 당시 범위에 인증·외부 의존성 readiness·TLS 재로드 트리거·관리 엔드포인트·CI는 포함하지 않았다. 현재 auth 연결은 [JWT API](../../auth/jwt-api/plan.md)를 따르며 풀 수치 경로는 아래 현재 구성에 기록한다. (확인)

### 현재 풀 수치 경로

현재 앱은 인증 없이 `GET /db_status`를 제공하며 HTTP 200과 `max_connections`·`size`·`idle`을 JSON으로 반환한다. HTTP·HTTPS 공통 Router에 등록되어 있다. 응답에는 별도 data 래퍼가 없다. 예를 들어 풀 최대 연결 수가 5이고 연결 1개가 유휴 상태라면 `{"max_connections":5,"size":1,"idle":1}` 형태다. (확인)

이 경로는 연결 획득·DB 쿼리 없이 풀 관측 수치를 읽는다. 수치의 원자적 일관성이나 DB readiness를 보장하지 않는다. 운영 노출·접근 제한 정책은 미확정이며 현재 공개 동작을 기록한 것으로 구분한다. (확인)

## 요구사항과 완료 조건

<a id="req-http-app-001"></a>
### REQ-HTTP-APP-001: 앱의 서버 조립

- 현재 빌드에서 `server.http`와 `server.https`는 모두 필수다. 블록 누락은 설정 역직렬화에서 거부한다. HTTP 블록에 TLS가 있거나 HTTPS 블록에 TLS가 없으면 서버 생성 전에 실행을 거부한다.
- 기본 설정으로 실제 앱이 요청을 받으며, HTTPS와 정적 파일을 설정하면 공통 Router를 사용한다.
- 실패·종료 시 이번 앱이 소유한 리스너를 정리한다. 미완료 TLS 핸드셰이크의 즉시 취소는 기존 라이브러리 계약대로 보장하지 않는다.
- 완료 조건: 설정 검증, 기본 기동, HTTP·HTTPS 실제 응답, TLS 준비 실패 시 미기동, 실행 실패 시 다른 리스너 해제, 종료 후 리스너 부재 확인.

<a id="req-http-app-002"></a>
### REQ-HTTP-APP-002: 생존 확인

- `/health`는 인증 없이 호출할 수 있는 생존 확인이다. `GET`은 200·`ok`, 자동 HEAD는 본문 없이 200이다.
- POST는 405, `/heath` 등 없는 경로는 404다. DB나 외부 서비스의 상태를 정상이라고 표현하지 않는다.
- 완료 조건: 실제 프로세스에 요청하여 상태 코드·헤더·본문과 실패 경로를 확인.

<a id="req-http-app-003"></a>
### REQ-HTTP-APP-003: 검증 진입점

- `just app-run`은 포그라운드 실행, `just http-health [base_url] [curl 옵션...]`은 실제 앱에 유한 시간 내 요청한다.
- 응답 본문과 HTTP 상태를 보여 주고 연결 실패·HTTP 4xx/5xx는 명령 실패로 전달한다. 정확한 200·`ok` 여부는 관찰 결과와 비교한다.
- Windows는 PowerShell의 curl 별칭을 피하도록 `curl.exe`를 사용한다. HTTPS 검증은 `--cacert`로 신뢰를 지정한다.
- 검증 도구·사용법, 도구와 독립적인 기능 기대 결과, 작업별 실제 실행 기록을 분리한다.

## 요청 추적 추가 제안

사용자가 제시한 `TraceparentData`, `new_request_span`, `get_current_traceparent` 예시와 `TraceLayer` 연결 요청을 기준으로 정리한다. 현재 앱의 `http.rs`에는 사용자가 추가한 `TraceLayer::new_for_http()` 변경이 있으며, 앱의 `tower-http` 의존성과 라이브러리의 `trace` feature는 아직 없다. 공통 tracing 초기화는 `Registry`를 사용한다. (확인)

### 단계별 진행 상태

- 요구사항 정리: 승인. 요구사항 제시 후 사용자의 “진행해” 응답을 승인 근거로 한다.
- 설계: 승인. 사용자의 “구현및 리뷰진행” 응답을 승인 근거로 한다. 후속 parser 제안에 따라 버전 `00`만 수용하는 `split` 기반 검증으로 조정했다.
- 구현·검증: 완료. 사용자의 후속 의견을 반영하여 새 요청 추적 검증은 `oneshot`으로 수행했다.
- 자체 점검·코드 리뷰: 사용자의 구현·리뷰 일괄 요청에 따라 완료. 아래 결과는 해당 미커밋 변경에 대한 평가이며, 문서 전체의 채택 상태와 구분한다.

<a id="req-http-app-004"></a>
### REQ-HTTP-APP-004: 요청별 추적 컨텍스트

- 수신 `traceparent`가 지원 버전 `00`의 유효한 값이면 trace ID를 이어받고, 수신 span ID를 요청 span의 `parent_span_id`로 기록한다. 다른 버전은 새 trace로 시작한다.
- 헤더가 없거나 유효하지 않으면 새 trace ID로 시작한다. 잘못된 추적 헤더 때문에 HTTP 요청을 거부하지 않는다.
- 각 요청은 새 span ID를 생성하며, trace ID와 span ID는 각각 32자리·16자리의 소문자 16진수이고 전체가 0인 ID를 생성하거나 수용하지 않는다.
- 새 trace의 로그상 부모 ID는 예시처럼 16자리 0으로 표시한다. 이 값은 유효한 외부 부모 ID와 구분한다.
- 완료 조건: 유효 헤더의 부모 연결, 누락·잘못된 헤더의 새 trace 생성, 요청별 span ID 분리를 검증한다.

<a id="req-http-app-005"></a>
### REQ-HTTP-APP-005: 요청 로그 연결

- 요청 span 이름은 `http_request`로 하고 `method`, `uri`, `trace_id`, `span_id`, `parent_span_id` 필드를 기록한다.
- 요청 시작·응답 상태·처리 시간은 TraceLayer가, 서버 오류 진단은 해당 route가 요청 span 안에서 기록한다. 기본 `info` 필터에서도 요청·응답 로그를 확인할 수 있게 한다. 상태 코드만으로 ERROR를 추가하지 않는다.
- 공통 기능은 `tlab::http::trace`가 제공하고, 앱이 조립한 Router에 `TraceLayer`를 적용한다. 기존 사용자 변경을 이어서 보완한다.
- 완료 조건: HTTP·HTTPS가 공유하는 Router의 핸들러 로그와 요청·응답 로그에서 동일한 요청 ID를 확인한다. 정상 응답과 4xx·5xx를 구분해 확인한다.

<a id="req-http-app-006"></a>
### REQ-HTTP-APP-006: 현재 요청 컨텍스트 조회

- `TraceparentData`는 `trace_id`, `span_id`를 제공한다.
- `get_current_traceparent()`는 현재 요청 컨텍스트를 반환하고, 요청 안에서 만든 자식 span에서도 조회할 수 있게 한다.
- 요청 컨텍스트가 없거나 subscriber가 지원하지 않거나 span이 필터링되어 컨텍스트를 보관하지 못하면 `None`을 반환한다. 추적 조회 때문에 panic을 발생시키지 않는다.
- 완료 조건: 요청 span·자식 span에서의 조회, 요청 밖·subscriber 부재 시의 안전한 반환을 검증한다.

이번 제안은 수신 컨텍스트와 로그 연결, 현재 ID 조회까지다. 응답 헤더 추가, 외부 HTTP 클라이언트의 헤더 자동 주입, `tracestate` 전달, OpenTelemetry exporter는 별도 요청이 있을 때 범위를 정한다. (추론)

### 요청 추적 설계

#### 책임과 공개 API

| 위치 | 제공할 동작 |
| --- | --- |
| `tlab/src/http/trace.rs` | `TraceparentData`, `new_request_span<B>(&Request<B>) -> tracing::Span`, `get_current_traceparent() -> Option<TraceparentData>` |
| `tlab/src/http.rs` | `pub mod trace`로 공통 API 공개. 리스너 실행 책임은 유지 |
| `tlab-app/src/http.rs` | 모든 route와 정적 Router를 조합한 뒤 공통 `TraceLayer` 적용. 요청·응답 로그 수준을 INFO로 선택 |
| Cargo 의존성 | 앱에 `tower-http` 0.7의 `trace` feature 추가. 라이브러리에 ID 생성용 `uuid`의 `v4` feature 추가 |

요청 추적은 재사용 가능한 HTTP 관심사이고, 적용 대상·로그 수준의 선택은 앱이 소유하므로 위처럼 배치한다. 기존 subscriber 초기화와 HTTP·HTTPS 서버 구현은 재사용한다. (추론)

#### 수신 헤더와 ID 생성

- `parse_traceparent`는 모듈 내부 함수로 둔다. 단일 헤더를 읽고 `version-trace_id-parent_id-flags`의 필수 영역을 검증한다. 중복 헤더는 모호한 입력으로 보고 새 trace로 시작한다.
- 사용자가 제시한 `split('-')` 방식으로 정확히 네 필드인지 확인하고, `TRACEPARENT_VERSION = "00"`, ID 검증, flags 검증을 한 조건식으로 표현한다. `is_hex`는 길이와 소문자 16진수를, `is_valid_id`는 이에 더해 전체 0이 아님을 확인한다. 버전 `00` 형식은 [W3C Trace Context](https://www.w3.org/TR/trace-context/#traceparent-header)를 따른다. 상위 버전의 수신 호환성은 지원하지 않고 새 trace로 시작하는 제한이 있다. (확인: 사용자 후속 제안 / 선택: 이번 지원 범위)
- 정상 헤더의 trace ID와 부모 span ID만 사용한다. flags는 형식만 확인하고 이번 로컬 로그의 필터나 sampling 판단에 사용하지 않는다.
- 새 trace ID는 UUID v4의 하이픈 없는 문자열로 만든다. 새 span ID는 별도의 UUID v4에서 16자리 16진수 부분을 취하고 전체 0이면 다시 생성한다. 요청마다 독립적으로 생성한다.
- 신규 trace의 `parent_span_id`는 `0000000000000000`이다. 외부 입력에서 같은 부모 ID가 오면 유효하지 않은 헤더로 처리한다.

#### span과 컨텍스트 조회

- `new_request_span`은 INFO 수준의 `http_request` span을 만들고 요구사항의 다섯 필드를 기록한다. `parent: None`으로 로컬 요청 span을 독립시켜, 호출 시 우연히 활성화된 다른 요청 span을 부모로 삼지 않는다. 수신 부모 관계는 `parent_span_id` 필드로 표현한다.
- 예시와 같이 `with_subscriber`와 `Registry`의 span extensions에 `TraceparentData`를 저장한다. span이 비활성 상태이거나 지원되지 않는 subscriber이면 저장 없이 반환한다.
- `get_current_traceparent`는 현재 span부터 부모 방향으로 탐색하고 처음 발견한 `TraceparentData`를 복제한다. extensions가 없는 일반 자식 span에서도 요청 ID를 찾는다. subscriber 부재·미지원은 `None`이다.
- 별도로 spawn한 task에는 tracing 컨텍스트가 자동 전달된다고 보장하지 않는다. 호출자가 `.instrument(...)` 등 기존 tracing 방식으로 span을 전달해야 한다.

#### 앱 연결과 로그

기존 사용자의 `TraceLayer::new_for_http()`에 `.make_span_with(tlab::http::trace::new_request_span)`를 연결하고 `DefaultOnRequest`와 `DefaultOnResponse`는 INFO로 명시한다. 후속 오류 로깅 분리 요청에 따라 `.on_failure(())`로 자동 실패 로그를 비활성화한다. 응답에는 status와 응답 헤더 생성까지의 latency가 기록된다. 스트리밍 본문 전체 전송 시간은 이번 처리 시간의 의미에 포함하지 않는다.

앱 `main`은 `initialize`의 반환 guard를 `_tracing_guard`로 보관한다. 기존 `let _ = ...`는 즉시 guard를 drop하므로, 요청 로그의 비동기 writer 수명을 유지하기 위해 보완한다. (확인)

4xx·5xx를 포함한 응답 상태는 INFO로 관찰하고, 오류의 의미를 아는 route가 진단 로그를 명시한다. `create_local_user`는 내부 실패를 ERROR, 예상된 용량 제한 `Busy`를 WARN으로 기록한다. status만 반환한 500에는 별도 ERROR가 없으며, TraceLayer의 service·body failure callback도 비활성 상태다. 새 실패 처리 경로 역시 해당 경계에서 필요한 로그를 책임진다. 기본 요청·응답 로그가 DEBUG인 점은 로컬 `tower-http` 0.7.1 소스에서 확인했으므로 INFO로 조정한다. (확인)

#### 검증 방법

- REQ-HTTP-APP-004: 공개 `new_request_span`을 통해 정상·누락·잘못된 헤더의 ID와 부모 필드, ID 형식과 요청 간 분리를 검증한다. 경계 입력은 잘못된 길이·대문자·0 ID·버전·flags·중복 헤더를 포함한다.
- REQ-HTTP-APP-006: 전역 subscriber를 변경하지 않는 격리된 Registry로 요청 span·자식 span의 조회, 요청 밖·비활성 span·미지원 subscriber에서 `None`을 확인한다. 동시 요청의 ID가 섞이지 않는지도 확인한다.
- REQ-HTTP-APP-005: 테스트 Router와 실제 TraceLayer를 연결하고 로그 writer를 격리해 응답 상태·latency·요청 ID와 INFO 필터의 요청·응답 기록을 검증한다. 로컬 사용자 오류 변환을 연결한 `oneshot` 테스트로 내부 실패의 ERROR 한 건, `Busy`의 WARN 한 건, 입력 오류·중복의 진단 로그 부재를 확인한다.
- `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`를 수행한다. 테스트 실행에 필요한 환경 문제는 기능 실패와 구분해 기록한다.
- 사용자의 후속 “검증은 oneshot 으로 가능하지 않아?” 의견을 반영해 실제 서버를 새로 실행하는 검증은 생략한다. 서버 조립 시 사용하는 동일한 `trace_requests` 함수와 `TraceLayer`를 테스트 Router에 적용해 `oneshot`으로 검증한다. 리스너·TLS 코드는 변경하지 않았으며 기존 HTTP·HTTPS 통합 테스트 결과를 재사용한다. (확인)

### 요청 추적 구현·검증 결과

- `tlab::http::trace`에 요청 span 생성·현재 컨텍스트 조회를 추가했다. parser는 사용자가 제시한 `split` 기반으로 구현했고, 버전 `00`만 수용한다. flags를 sampling 판단에 사용하지 않는다.
- 앱의 기존 TraceLayer 변경을 보완해 요청·응답 INFO, 서버 오류 ERROR와 요청 ID를 연결했다. `trace_requests`는 실제 앱과 `oneshot` 테스트에서 함께 사용한다.
- tracing guard를 main 종료까지 보관하도록 수정했다. DB·설정·응답 본문·HTTP·HTTPS 리스너 계약은 변경하지 않았다. (확인)

| 실행 | 결과와 범위 |
| --- | --- |
| `cargo test --workspace --locked` | 통과. 라이브러리·앱·doc tests와 기존 HTTP·HTTPS 통합 테스트 포함. 기존의 격리 실행용 1개와 PostgreSQL 연동 6개는 기본 설정대로 ignored |
| `cargo test -p tlab-app http::tests --locked` | 최종 테스트 보완 후 2개 통과. `oneshot`으로 200·404·500과 헤더 누락·오류·미지원 버전, 로그의 부모 ID와 trace/span ID, 동시 요청 분리를 확인 |
| `tlab/tests/http_trace.rs` | 워크스페이스 실행에서 4개 통과. 정상 헤더·flags, 0 ID·대문자·잘못된 길이·버전·중복 헤더, 자식 span 조회, 독립된 요청 root, subscriber 부재·미지원·필터링을 확인 |
| `cargo fmt --all -- --check` | 최종 변경에서 통과 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 최종 테스트 보완 후 통과 |
| `cargo audit` | 종료 코드 0, 차단 advisory 없음. 최종 코드와 동일한 Cargo.lock 대상으로 실행 |
| `git diff --check` | 통과 |

워크스페이스 전체 테스트 후 앱의 `oneshot` 로그 테스트에 헤더 누락·오류·미지원 버전을 보완했다. 그 이후 영향받은 앱 테스트와 fmt·clippy를 재실행했다. tracing 초기화·파일 writer의 수명 계약은 기존 `tlab/tests/tracing.rs` 검사 결과를 재사용한다. (확인)

별도 앱 프로세스의 실행 검증은 로컬 포트 바인딩 권한에서 차단되었고, 권한 확대 요청은 사용자가 중단했다. 이후 사용자 의견에 따라 `oneshot` 검증으로 범위를 조정했으며 실제 프로세스 검증을 통과한 것으로 기록하지 않는다. PostgreSQL 연동 ignored 테스트는 이번 변경 대상이 아니므로 별도로 실행하지 않았다. (확인)

### 요청 추적 코드 리뷰

- 대상: `f73e219` 대비 이번 요청 추적의 미커밋 변경. 시작 시 존재한 앱 TraceLayer 변경을 보존·완성한 부분, 신규 trace 모듈·테스트, 의존성·잠금 파일, tracing guard와 본 명세를 포함한다.
- 선행 검사: 위 fmt → clippy → audit 순서로 통과 후 LLM 리뷰 수행. 최종 테스트 보완 후 fmt·clippy 결과를 갱신했으며, 잠금 파일이 같아 audit 결과는 재사용했다.
- 실행 환경: rustc 1.97.1, cargo 1.97.1, cargo-audit 0.22.2, 기본 feature·호스트 target. RustSec DB commit `309392b18f6b17022f82c701f300433126901068`, 이번 세션에서 갱신·실행.
- Miri: unsafe·메모리 조작이나 해당 안전성 전제 변경이 없어 비대상. Geiger: 참고 도구로 실행하지 않았으며 의존성에 unsafe가 없다고 판단하지 않는다.
- 발견 사항: 수정이 필요한 결함 없음. 헤더 검증, async 요청 span의 범위, 자식 span 탐색, 필터링·subscriber 실패 경로, 기존 사용자 변경 보존을 확인했다. (확인)
- 판정: 프로젝트 가이드의 초기 제안 기준으로 통과, 100/100. 이 점수는 아래 제한된 변경 범위의 항목별 판단이며 별도 독립 평가자의 리뷰나 운영 성능 측정 결과는 아니다. (추론)

| 항목 | 점수(0~4) | 근거 |
| --- | ---: | --- |
| 기능 | 4 | 지원 버전·입력 경계·부모 연결·자식 조회·로그와 응답 상태를 공개 API 및 실제 layer로 확인 |
| 가독성과 구조 | 4 | HTTP 공통 기능과 앱 적용 책임 분리, 사용자 제안 parser 반영, 서버·subscriber 초기화 구조 재사용 |
| 안정성 | 4 | 요청별 새 span과 독립 root, 동시 요청 격리, subscriber 부재·필터링 안전 반환, guard 수명 보완 |
| 보안 | 4 | ID·flags의 길이·문자 검증, 0 ID·모호한 중복 헤더 거부, 수신 flags로 로그 필터를 바꾸지 않음. 요청 본문·전체 헤더 로깅 추가 없음 |
| 성능 | 4 | 고정 네 필드 분리, 유효성 실패 시 문자열 복제 생략, 요청당 ID 생성과 현재 span 깊이에 따른 컨텍스트 탐색. 추가 네트워크·DB I/O 없음 |
| 테스트 용이성 | 4 | 실제 앱의 layer 적용 함수를 재사용하며 Registry와 writer를 격리. DB·서버 프로세스 없이 입력·출력·동시성 계약 재현 |

- 복구 가능성: 이전 코드를 복원하면 동작 복구 가능. DB 스키마·저장 데이터·외부 상태 변환이 없고 새 필수 설정도 없어 별도 보정이 필요하지 않다. 기존에 기록된 로그는 유지되며 실제 롤백 실행은 하지 않았다. (추론)
- 아키텍처 변경: 없음. `tlab-app → tlab` 의존 방향과 Router 조립·서버 실행 책임을 유지하면서 선택적인 HTTP 보조 API를 추가했다. 기존 소비자 전환이나 배포 순서 변경은 필요하지 않다. (추론)
- 리스크: R1. 범위는 요청 로그와 로컬 컨텍스트이며 코드 복원으로 동작을 되돌릴 수 있다. (추론)
- 제한: 버전 `00`만 지원하며 출력 헤더·외부 요청 전파는 제공하지 않는다. 비활성 요청 span에서는 컨텍스트 조회가 `None`이고, 새 task에는 호출자가 span을 전달해야 한다. latency는 응답 헤더 준비까지이며 스트리밍 전송 완료 시간이 아니다.

### 후속 변경: 오류 로그를 route 경계로 이동

- 사용자 “변경해”와 후속 원인 분류기 제거 의견을 반영했다. 위 구현·리뷰 결과의 자동 5xx ERROR는 초기 구현에 대한 이력이며, 현재 동작은 본문의 `on_failure(())`와 route의 명시적 로깅을 따른다.
- status만 반환한 500에는 INFO 응답 로그만 남는다. 로컬 사용자 오류 변환을 연결한 `oneshot` 테스트에서는 내부 실패의 ERROR 한 건과 Busy의 WARN 한 건, 입력 오류·중복의 진단 로그 부재를 확인했다. 모든 이벤트가 요청 trace ID와 연결된다. 후속 cause 로깅 요청에 따라 내부 실패의 원인은 route의 ERROR에 기록하고 HTTP 응답에서는 숨긴다.
- 최종 앱 테스트 12개 통과·PostgreSQL 연동 1개 ignored, fmt·clippy 통과. 작업별 승인·검증·점검은 [로컬 사용자 추가 변경 기록](../../auth/create-local-user/plan.md#요청-tracing에-따른-오류-로그-책임-정리)을 참조한다. (확인)

## 구성

`main.rs`의 설정 로딩 → tracing 초기화 → `app::Container::new`의 구성요소 준비 → `http.rs`의 auth·`/health`·`/db_status`·정적 Router 조립 → 두 서버의 `run(router)`·종료 순서다.
`app.rs`는 설정 타입과 Container를 제공한다. Container는 설정을 소유하고 서버를 생성하며, 앱 HTTP 모듈은 Container의 설정을 참조해 Router를 조립하고 실행·종료를 관리한다. 설정 전체를 복제하지 않고 Container로 이동한다. (확인)
`http::run()`은 두 서버의 run과 OS 신호 대기를 `try_join!`으로 함께 기다린다. 신호를 받으면 각 Server의 `graceful_shutdown()`을 호출하고 run 완료까지 기다린다. main은 설정·tracing 초기화와 실행 경로 선택을 담당하고 서버 실행은 `http::run()`에 위임한다. watch 채널이나 run의 shutdown 인자는 사용하지 않는다. 실행·신호 오류 시 나머지 future를 취소한다. OS 신호 종류는 `http::shutdown_signal()`에서 선택한다. (확인)

기존 `TLAB_CONFIG_FILE`과 `TLAB_APP_` 환경변수 규칙을 유지한다. 상대 경로는 실행 작업 디렉터리 기준이다.
예를 들어 `TLAB_APP_SERVER__HTTP__BIND`로 HTTP 주소를 덮어쓴다. 실행·검증 사용법은 [HTTP 도구](../../../verification/tools/http/README.md)에 정리한다. (확인)

## 구현·변경 기록

### 1차 구현

- 사용자 요청과 `/health` 경로 확인을 반영해 앱·기본 설정·just 명령을 연결했다.
- HTTP·HTTPS 선택과 신호 처리는 앱에서 수행하고 라이브러리는 변경하지 않았다.
- 복잡한 요청 시나리오가 없어 Python 패키지·전용 검증 프레임워크는 추가하지 않았다.
- 실제 실행·검사와 남은 범위는 [검증 기록](./verification.md)에 남긴다. (확인)


### 서버 API 정리 반영

- HTTP 모듈 정리에 맞춰 `Server::new`로 모든 TLS를 준비한 뒤 `run(router)`을 함께 기다리도록 변경했다. 바인딩 실패 시 앱의 `try_join!`이 다른 실행을 취소한다. 모든 포트를 미리 확보하는 기존 보장은 제거했다.
- “application starting”과 실제 리스너 바인딩 로그를 구분했다. 공통 Router·신호 처리·설정 선택은 유지했다.
- 워크스페이스 테스트에서 앱 설정 테스트와 HTTP·HTTPS 통합 테스트를 확인했다. 후속 앱 적용 요청에 따라 실제 바이너리의 HTTP·HTTPS·정적 파일 응답과 SIGTERM 종료도 확인했다. 상세는 [실행 기록](./verification.md#newrun-적용-후-실제-앱-확인)을 참조한다. (확인)

### 앱 종료 대기 정리

- 사용자 의견에 따라 `main()`의 서버 실행과 신호 대기를 경쟁시키던 `select!`와 `running` future의 pin을 제거했다. 기존 watch 알림을 보내는 future를 서버 실행과 함께 `try_join!`에 넣었다. 추가 task·의존성·공개 API 변경 없이 종료 알림과 완료 대기를 유지한다. (확인)


### 명시적 종료 요청

- 사용자 후속 의견에 따라 종료 요청을 상위에 드러냈다. main이 각 서버의 graceful_shutdown()을 직접 호출하며 run에는 Router만 전달한다. watch 채널을 제거했다. (확인)

### 앱 Container 정리

- 사용자의 Container 구조 개선 요청과 후속 “http/https는 필수” 지시를 반영했다. 현재 두 설정·서버 필드를 필수로 유지하며, 향후 선택은 컴파일 시점에 처리할 의도다. 컴파일 조건 자체는 이번에 추가하지 않았다. (확인)
- `app::Container`가 설정 소유·프로토콜 검증·서버 생성을 맡고, main은 Router 조립·실행·종료를 맡는다. 서버 생성 후 설정 원본을 Container에 이동해 중복 복제를 제거했다. (확인)
- 기존 테스트를 app 모듈로 옮기고 필수 설정 누락, TLS 프로토콜 불일치, 기본 HTTP·HTTPS 설정을 확인하도록 변경했다. 검증 결과는 [검증 기록](./verification.md#앱-container-정리-검증)에 남긴다. (확인)

### HTTP-01~05: 현재 앱 책임과 공개 경로 보완

사용자 요청으로 이동한 auth 변경 기록 링크를 수정하고 실행·종료 조정의 주체를 http::run으로 맞췄다. 현재 인증 없는 /db_status의 JSON 필드·쿼리 없는 풀 관측·readiness 미보장과 미확정 운영 접근 정책을 문서화했다. 과거 main 배치·검증 결과는 당시 기록으로 보존했다. 문서·소스 대조와 링크·diff 검사를 수행했으며 접근 정책이나 라우팅 동작을 바꾸지 않았다. (확인)

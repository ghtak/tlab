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

상태 본문·기본 포트·종료 방식은 이번 구현의 선택이다. 인증·외부 의존성 readiness·TLS 재로드 트리거·관리 엔드포인트·CI는 이번 범위에 추가하지 않는다. (추론·선택)

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

- `just app-run`은 포그라운드 실행, `just app-health [base_url] [curl 옵션...]`은 실제 앱에 유한 시간 내 요청한다.
- 응답 본문과 HTTP 상태를 보여 주고 연결 실패·HTTP 4xx/5xx는 명령 실패로 전달한다. 정확한 200·`ok` 여부는 관찰 결과와 비교한다.
- Windows는 PowerShell의 curl 별칭을 피하도록 `curl.exe`를 사용한다. HTTPS 검증은 `--cacert`로 신뢰를 지정한다.
- 검증 도구·사용법, 도구와 독립적인 기능 기대 결과, 작업별 실제 실행 기록을 분리한다.

## 구성

`main.rs`의 설정 로딩 → tracing 초기화 → `app::Container::new`의 프로토콜 검증·두 `Server::new` → `http.rs`의 `/health`·정적 Router 조립 → `run(router)`·종료 순서다.
`app.rs`는 설정 타입과 Container를 제공한다. Container는 설정을 소유하고 서버를 생성하며, 앱 HTTP 모듈은 Container의 설정을 참조해 Router를 조립하고 실행·종료를 관리한다. 설정 전체를 복제하지 않고 Container로 이동한다. (확인)
`main()`은 두 서버의 run과 OS 신호 대기를 try_join으로 함께 기다린다. 신호를 받으면 각 Server의 graceful_shutdown()을 호출하고 run 완료까지 기다린다. watch 채널이나 run의 shutdown 인자는 사용하지 않는다. 실행·신호 오류 시 나머지 future를 취소한다. OS 신호 종류의 선택만 shutdown_signal() 내부에서 처리한다. (확인)

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

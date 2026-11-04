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
앱이 설정 선택·Router 조립·다중 인스턴스 실행·종료를 맡고 `tlab::http`의 API는 변경하지 않는다. (확인)

| 항목 | 이번 선택과 이유 |
| --- | --- |
| 상태 확인 | `GET /health` → 200, `text/plain; charset=utf-8`, 본문 `ok`. 외부 DB·서비스를 검사하지 않는 생존 확인 |
| 기본 실행 | `127.0.0.1:8080`의 HTTP만 활성화. HTTPS·정적 파일은 명시적으로 설정할 때 연결 |
| 설정 | 앱의 `server.http`, `server.https`에 각각 `http::Config`, 공통 `server.static_files`에 `StaticFilesConfig` 포함 |
| 그룹 준비 | 활성 인스턴스를 모두 준비한 다음 실행. 준비 실패 시 앞서 준비한 객체도 drop |
| 그룹 실행 | 같은 Router를 전달하고 `try_join!` 사용. 한 실행의 실패는 다른 실행 future를 drop하고 앱 오류로 전달 |
| 종료 | 앱이 Ctrl+C, Unix에서는 SIGTERM도 받음. 공통 종료 신호를 각 인스턴스에 전달하고 완료를 기다림 |
| 검증 도구 | `just`는 명령 진입점, `curl`은 요청·응답 관찰. 다중 요청·상태·JSON 검증이 필요해질 때 Python `requests` 추가 |

상태 본문·기본 포트·종료 방식은 이번 구현의 선택이다. 인증·외부 의존성 readiness·TLS 재로드 트리거·관리 엔드포인트·CI는 이번 범위에 추가하지 않는다. (추론·선택)

## 요구사항과 완료 조건

<a id="req-http-app-001"></a>
### REQ-HTTP-APP-001: 앱의 서버 조립

- 앱이 HTTP 단독·HTTPS 단독·동시 설정을 표현할 수 있다. HTTP 블록에 TLS가 있거나 HTTPS 블록에 TLS가 없거나 두 블록이 모두 없으면 실행을 거부한다.
- 기본 설정으로 실제 앱이 요청을 받으며, HTTPS와 정적 파일을 설정하면 공통 Router를 사용한다.
- 실패·종료 시 이번 앱이 소유한 리스너를 정리한다. 미완료 TLS 핸드셰이크의 즉시 취소는 기존 라이브러리 계약대로 보장하지 않는다.
- 완료 조건: 설정 검증, 기본 기동, HTTP·HTTPS 실제 응답, 두 번째 준비 실패 시 첫 포트 해제, 종료 후 리스너 부재 확인.

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

`main.rs`의 앱 설정 → 선택 검증 → tracing 초기화 → `/health`·정적 Router 조립 → 각 `Server::bind` → 실행·종료 순서다.
공통 watch 신호를 종료 future로 전달한다. 라이브러리에 다중 서버 관리 기능이나 전역 신호 처리를 넣지 않는다. (확인)

기존 `TLAB_CONFIG_FILE`과 `TLAB_APP_` 환경변수 규칙을 유지한다. 상대 경로는 실행 작업 디렉터리 기준이다.
예를 들어 `TLAB_APP_SERVER__HTTP__BIND`로 HTTP 주소를 덮어쓴다. 실행·검증 사용법은 [HTTP 도구](../../../verification/tools/http/README.md)에 정리한다. (확인)

## 구현·변경 기록

### 1차 구현

- 사용자 요청과 `/health` 경로 확인을 반영해 앱·기본 설정·just 명령을 연결했다.
- HTTP·HTTPS 선택과 신호 처리는 앱에서 수행하고 라이브러리는 변경하지 않았다.
- 복잡한 요청 시나리오가 없어 Python 패키지·전용 검증 프레임워크는 추가하지 않았다.
- 실제 실행·검사와 남은 범위는 [검증 기록](./verification.md)에 남긴다. (확인)

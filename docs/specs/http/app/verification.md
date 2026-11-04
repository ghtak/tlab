---
id: VER-HTTP-APP
status: draft
---

# 앱 HTTP 연결 검증

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| verifies | [SPEC-HTTP-APP](./spec.md) | 앱 기동·상태 확인·설정·종료 |

## 범위와 방법

실제 `tlab-app`을 기동하고 `just app-health`와 직접 curl 요청으로 관찰한다.
기능·기대 결과는 [Feature map](../../../verification/feature/http-health.md), 도구·실행·격리는 [HTTP 도구](../../../verification/tools/http/README.md)를 따른다.
단순 요청으로 검증 가능하여 Python `requests` 패키지·시나리오는 추가하지 않았다. (확인)

기본 실행은 `just app-run`으로 시작하고 다른 터미널에서 `just app-health`를 호출한다.
실제 상태·본문·헤더는 직접 curl로 확인하며 종료는 시작한 앱에만 전달한다.
HTTPS·정적 파일·실패 검증에는 `/tmp/tlab-app-http-j355gh4l`의 별도 설정·로그·인증서와 18080·18443·18082 포트를 사용했다. (확인)

## 실제 수행·관찰

2026-11-04, Darwin arm64, Rust/Cargo 1.97.1, just 1.57.0, curl 8.7.1에서 미커밋 변경을 검증했다.
최초 실행·요청은 샌드박스의 loopback 제한으로 실패했으며, 승인 후 실제 프로세스와 연결을 다시 실행했다.
아래 응답은 승인 후 관찰 결과다. (확인)

| 항목 | 수행과 관찰 | 판정 |
| --- | --- | --- |
| 기본 앱 | `just app-run`에서 127.0.0.1:8080 준비·기동 로그. `just app-health` → `ok`, `HTTP 200` | 통과 |
| GET 계약 | `curl --include --max-time 5 http://127.0.0.1:18081/health` → 200, `content-type: text/plain; charset=utf-8`, 본문 `ok` | 통과 |
| HEAD | 같은 주소의 `curl --head --max-time 5` → 200, 본문 없음 | 통과 |
| 실패 경로 | 기본 인스턴스에서 POST `/health` → 405, GET `/heath` → 404 | 통과 |
| 환경변수 | `TLAB_APP_SERVER__HTTP__BIND=127.0.0.1:18081 just app-run`, 해당 포트의 `just app-health` → 200·`ok` | 통과 |
| HTTP·HTTPS 동시 | 임시 설정으로 18080·18443 기동, 각 `just app-health` → 200·`ok` | 통과 |
| TLS 신뢰 | `just app-health https://localhost:18443 --cacert <임시 chain.pem>` → 200·`ok`. 인증서 검증 생략 옵션 미사용 | 통과 |
| 공통 정적 파일 | 양쪽 `/static/hello.txt`를 curl로 요청 → 200·`static-ok` | 통과 |
| 부분 준비 실패 | HTTP 18082 준비 후 HTTPS가 기존 8080과 충돌 → 앱 종료 코드 1, 18082 리스너 부재 | 통과 |
| 모두 비활성화 | `[server]`만 있는 임시 설정으로 앱 실행 → `InvalidInput`, 종료 코드 1 | 통과 |
| 설정 정책 | 앱 단위 테스트에서 HTTP TLS 혼입·HTTPS TLS 누락·인스턴스 없음 거부, HTTP·HTTPS 단독 허용 | 통과 |
| Ctrl+C | 기본·포트 변경 실행에 Ctrl+C → 앱 종료 로그·리스너 해제. just 래퍼는 SIGINT 중단 코드 130 | 통과 |
| SIGTERM | 이번에 시작한 이중 인스턴스 앱 PID 34840에 TERM → 종료 로그, 래퍼 종료 코드 0, 양쪽 리스너 해제 | 통과 |
| 종료 후 요청 | 종료된 8080의 `just app-health` → curl 연결 실패, 종료 코드 7 | 통과 |

정확한 HTTP 상태·본문은 명령의 종료 코드와 별도로 확인했다. `/health` 성공으로 DB·외부 서비스의 readiness까지 판정하지 않았다. (확인)

## 코드·도구 검사

| 명령 | 결과 |
| --- | --- |
| `cargo check --workspace --offline` | 통과 |
| `cargo build -p tlab-app --locked --offline` | 통과 |
| `cargo test --workspace --locked --offline` | 61개 통과, 기존 ignored 5개, 실패 없음 |
| `cargo fmt --all -- --check` | 통과 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 통과 |
| `cargo audit` | 1,403개 advisory·266개 의존성 검사, 종료 코드 0 |
| `just --list`, `just --fmt --check` | 통과 |

기존 ignored 5개는 PostgreSQL 실연동 4개와 부모 테스트가 별도 프로세스로 실행하는 환경변수 보조 테스트 1개다.
실제 Windows 실행·부하·앱 TLS 재로드 트리거는 검증하지 않았다. Windows용 curl 실행 파일 선택과 Ctrl+C 분기는 코드에 반영했지만 실기기 결과로 보고하지 않는다. (확인)

## 정리·근거

이번에 시작한 앱 3개를 종료했다. 8080·18080·18081·18443·18082의 리스너 조회 결과가 모두 비어 있었다.
임시 설정 3개·생성한 인증서/키·정적 fixture는 삭제했고, 임시 실행 로그는 `/tmp/tlab-app-http-j355gh4l/logs`와 `collision-logs`에 남겼다.
기본 실행의 `./logs`와 이 문서의 명령·관찰 기록도 보존한다. 다른 프로세스·데이터는 종료하거나 삭제하지 않았다. (확인)

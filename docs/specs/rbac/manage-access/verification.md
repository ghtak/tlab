---
id: VER-RBAC-MANAGE-ACCESS
status: draft
---

# RBAC 07/08 실행 검증

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| verifies | [역할·퍼미션 관리](plan.md#rbac-07-08-current) | 현재 계약과 구현 결과 |
| 참고 | [로컬 DB 도구](../../../verification/tools/database/README.md) | 기존 PostgreSQL 환경 |
| 참고 | [테스트 가이드](../../../architecture/guide-test.md) | 기존 도구 재사용·실행 범위 구분 |

## 실행 경로와 기대 결과

원본 workspace에서 실행했다. 다른 에이전트·임시 앱 사본은 사용하지 않았다. herdr의 `rbac-0708` 터미널을 오른쪽 pane에 표시해 명령과 결과를 남겼다. DB 검증은 기존 PostgreSQL 환경의 UUID 고유 스키마와 해당 스키마만의 search_path를 사용하고 실패 후에도 정리한다. 개발/운영 업무 데이터에는 migration이나 bootstrap을 실행하지 않았다. (확인)

| 검증 경로 | 확인한 계약 |
| --- | --- |
| [원본 앱 HTTP 테스트](../../../../tlab-app/src/rbac/tests.rs) | `Container::new`와 서버가 사용하는 `http::router`를 직접 사용. JSON 로그인→가입 인가→역할 부여/회수→갱신, 기존 JWT 역할 잔존과 현재 역할 퍼미션 즉시 반영, 역할 편집·부여의 권한 상승 거부 |
| 같은 원본 Router | 인증 없음 401, 권한 없음 403, 성공 200/201/204, 미존재 404, version 충돌 409, resolver DB 장애 500과 비공개 오류 원인 미노출 |
| 같은 원본 웹 Router | Secure/HttpOnly/SameSite 쿠키 로그인, 웹 RBAC·가입, Origin 없는/다른 출처 쓰기 403, Refresh만으로 같은 요청의 최신 역할 발급·가입 성공, JSON/웹 인증 소스 분리, 역할 조회 장애 시 handler 미실행·기존 쿠키 유지 |
| 실제 HTTP 경합 | 서로 다른 계정의 역할 편집과 관리 요청. 편집을 테스트 trigger로 잠시 멈추고 `pg_blocking_pids`로 실제 역할 잠금 대기를 확인. 커밋 후 관리 권한 제거 또는 부여 대상의 퍼미션 확대를 다시 읽어 403, 금지된 역할/부여 행 미생성 |
| 05 계정 상태 보완 | 관리 퍼미션을 유지한 허용 계정의 성공과 탈퇴 계정의 403·미변경 대조. 기존 저장소 시나리오의 거짓 양성도 수정 |
| [실제 CLI 프로세스](../../../../tlab-app/tests/bootstrap_admin_cli.rs) | stdin 비밀번호 공백 보존·실제 Argon2 검증, 전체 현재 카탈로그(추가 코드 포함) 저장, ID 두 개만 stdout 출력. 기존 역할/이메일 재실행 충돌, 마지막 쓰기 실패 시 다섯 저장소 전체 rollback, stdout/stderr에 비밀번호·PHC·DB URL 미노출 |
| CLI 실행 경계 | TLS/JWT 파일 생성이 꺼지고 파일이 없는 임시 cwd에서도 성공, 로그·키 파일 미생성·프로세스 종료. 옵션 테스트로 migrate/서버와 상호 배타 선택·비밀번호 argv 옵션 거부 확인 |
| [기존 RBAC 테스트](../../../../tlab-app/tests/rbac_storage.rs) | 저장 제약·원자성·버전·인가·역할 부여/회수 경합·계정 일반 조회·bootstrap·404 회귀. 기존 테스트용 모듈 조립은 보존하며 원본 앱 HTTP 검증과 구분 |

이 결과는 Axum 요청/응답을 통한 실제 앱 통합과 별도 OS CLI 프로세스 검증이다. HTTP/HTTPS 리스너의 네트워크·브라우저 실제 동작은 이번 RBAC 검증에 포함하지 않는다. 기존 인증 테스트의 사용자 준비는 신뢰된 가입 usecase를 사용하고, 가입 HTTP 테스트에는 실제 bootstrap 계정의 권한을 연결했다. (확인)

## 실행 명령·결과

rustc/cargo 1.97.1, Clippy 0.1.97, 기본 feature·host target·기존 Cargo.lock을 사용했다. (확인)

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
env TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab-app --locked -- --include-ignored
env TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab --test migration --test sqlxdb --locked -- --include-ignored
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo audit
git diff --check
```

- 원본 앱: **79 passed**, CLI 프로세스: **1 passed**, RBAC 저장소: **26 passed**. 실패·ignored 없음. 원본 앱 79개에는 새 HTTP 시나리오 4개와 기존 인증·가입·설정·응답·요청 tracing 회귀가 포함된다. (확인)
- workspace 기본 테스트: 통과. DB ignored는 위 명령으로 별도 실행했으며 기본 실행의 ignored를 통과로 세지 않는다. 라이브러리 migration **1 passed**, sqlxdb **7 passed**. (확인)
- fmt·Clippy(`-D warnings`)·diff 검사: 통과. Rust 의존성·migration·Cargo.lock 변경 없음. (확인)
- audit: **종료 코드 1**, 1453 advisories·306개 의존성에서 기존 `libsqlite3-sys 0.37.0`의 **RUSTSEC-2026-0459** 1건. 도구 제시 수정 버전은 `>=0.39.0`. 무시 설정이나 의존성 변경을 추가하지 않았고 사용자 지정대로 결과를 기록하고 작업을 계속했다. (확인)

최초 `cargo test --workspace -- --include-ignored` 실행은 설정 테스트의 자식 프로세스 전용 `environment_scenario`를 환경 없이 직접 실행해 실패했다. 기본 workspace 명령은 해당 보조 테스트를 기존 자식 프로세스 호출 경로로 검증하며 통과했다. 최초 앱 실행의 새 테스트에서 빈 Cookie 헤더·역할 상세 응답 해석 오류, 기존 Busy 검증의 준비 경로 문제도 수정하고 최종 전체 앱 검사를 다시 실행했다. 해당 실패를 생산 정책 변경으로 해결하지 않았다. (확인)

## 자체 점검과 남은 한계

실제 diff·공개 경로/메서드·입력/응답·공용 AppResult·auth facade·Container 재사용·문서 링크를 대조했다. 가입 진입 인가는 route, 쿠키/CSRF는 auth 웹 전송 계층, 관리 최종 인가·역할 잠금·tx는 기존 RBAC usecase, 준비·해싱·계정 저장은 auth 공용 작업의 책임으로 유지한다. 생성 검증·임의 상한·별도 오류 추상화·동기화 절차·bootstrap advisory lock/카탈로그 재검증을 추가하지 않았다. (확인)

bootstrap·역할 변경 후 저장 상태는 코드 복원만으로 되돌아가지 않는다. 마지막 관리자·자기 회수 보호는 없으며 접근 소실 복구는 기존 합의대로 DB 보정이 필요하다. 운영 적용·보정/롤백 리허설·관찰/중단 기준의 실운영 검증, commit 중 통신 단절, 부하 시험은 수행하지 않았다. 이 한계를 테스트 통과로 해소했다고 주장하지 않으며 문서 상태는 draft로 유지한다. 커밋·배포는 하지 않았다. (확인·추론)

---
id: PLN-AUTH-JWT-LOGIN
status: implemented
---

# 로그인 발급 설계와 검증

## 책임과 계약

기존 `LocalLogin::execute`는 계정만 반환하는 HTTP 계약을 유지한다. `execute_with_tokens`는 같은 비밀번호 인증과 오류 정책을 사용한 다음 별도 트랜잭션에서 계정 행을 잠그고 현재 상태를 재확인한다. 탈퇴 계정은 거부한다. 현재 계정 모델에는 정지 상태가 없다. 비밀번호 검증은 잠금 이전에 수행한다. 계정 상태를 변경하고 Refresh를 폐기하는 후속 작업도 같은 계정 잠금 순서를 지켜야 직렬화된다. (확인)

잠금 획득 후 초 단위 UTC 시각을 정하고 UUID v4 sid, 256비트 Refresh를 생성한다. Refresh 원문은 성공 결과에서만 제공하며 SHA-256만 저장한다. 세션 생성·Refresh 생성 시각은 같고 두 만료는 최초 시각 + 168시간이다. Access는 동일한 시각부터 600초다. 저장 후 서명하고 commit 성공 후에만 토큰을 반환한다. 실패나 취소 시 미완료 트랜잭션은 rollback된다. commit 통신 실패는 DB 완료 여부를 알 수 없지만 성공 결과는 반환하지 않는다. (확인)

클레임 `sub`는 서버 계정 ID의 문자열, `user`는 서버 계정의 `nickname`과 `status`만 포함한다. 권한 원본이 없으므로 roles는 넣지 않는다. 입력은 기존 email과 password만 허용한다. Refresh 교체는 수행하지 않는다. (확인)

API 연결에서 Container가 `Ed25519KeyFile`과 `AccessIssuer`·`AccessVerifier`를 시작 시 준비한다. HTTP 계약은 [JWT API](../jwt-api/plan.md)를 따른다. 쿠키·CSRF와 키 운영은 후속 범위다. (확인·추론)

## 검증과 변경 기록

- 실제 PostgreSQL 고유 임시 스키마에서 기존 HTTP 계약과 실제 Ed25519 서명·공개키 검증, 서버 user, 두 로그인 sid·Refresh 독립성, 해시 조회·원문 미저장 및 600초/168시간을 확인했다. (확인)
- 서명 오류는 테스트 전용으로 잘못된 서명 키를 가진 실제 발급기를 구성해 재현한다. 로그인 usecase에는 테스트용 서명 함수 경계를 두지 않는다. 저장 실패는 INSERT 트리거, commit 실패는 지연 제약 트리거를 사용해 재현하고 성공 결과 없음과 저장 행 rollback을 확인했다. (확인)
- 탈퇴 트랜잭션이 계정 행을 잠근 동안 로그인 잠금 대기를 PostgreSQL 차단 관계로 확인한 후 탈퇴를 commit하여 계정 재확인과 새 Refresh 미생성을 확인했다. 정지 상태와 후속 폐기 usecase는 아직 없어 그 경로는 검증하지 않았다. (확인)
- `TLAB_TEST_DATABASE_URL=<로컬 개발 DB> cargo test -p tlab-app --locked --offline -- --include-ignored`: 28개 통과. 운영·기존 사용자 데이터는 변경하지 않았으며 테스트 스키마를 정리했다. (확인)
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo audit`: 통과. (확인)
- `cargo test --workspace --locked --offline`, `cargo fmt --all -- --check`, `git diff --check`: 통과. workspace 기본 실행에서 ignored인 라이브러리 DB 테스트는 실행하지 않았으며, 앱의 DB 테스트 7개는 위 별도 실행에 포함했다. (확인)
- 서명 테스트 구성을 변경한 뒤 격리 PostgreSQL의 `local_login_http_contracts`, workspace Clippy, fmt 및 diff 검사를 다시 실행해 통과했다. 위 전체 workspace·앱 테스트와 audit은 변경 전에 실행했다. (확인)

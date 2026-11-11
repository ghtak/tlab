# 이어서 할 작업

재개에 필요한 현재 상태다. 과거 구현·검증 이력은 작업 문서에 보존한다.

- **작업 문서:** [RBAC 06 현재 구현·재개 기준](specs/rbac/manage-access/plan.md#rbac-06-current). 후속 사용자 피드백을 반영한 본문과 하단 변경 기록을 따른다.
- **구현 상태:** 01~05와 06 코드·자체 점검을 완료했다. 06의 조회·부여·회수·유효 퍼미션 handler, bootstrap usecase, auth 가입 준비·해싱·저장 공용화가 있다. 06은 인계 단위이며 실행 앱 통합 완료는 아니다. (확인)
- **현재 계약:** 일반 조회는 잠금·tx가 없고 현재 DB 역할을 표시한다. 유효 퍼미션은 같은 연결의 `resolve_in_context`로 합산한다. 부여·회수는 계정 존재·상태를 일반 조회로 확인하고 계정 행을 잠그지 않는다. 역할 정렬 잠금·같은 tx의 현재 권한 재판정은 유지한다. bootstrap은 격리된 단독 실행이며 advisory lock·필수 카탈로그 코드 검사를 사용하지 않고 현재 카탈로그 전체를 QueryBuilder로 batch 저장한다. (확인)
- **로컬 계정 공용 API:** `prepare_local_account(ctx, policy, input)` → 연결 반환 → `hash_local_account(input, hasher, slots)` → 저장 tx의 `insert_prepared_local_account(ctx, prepared)`. 가입과 bootstrap이 공유하며 해싱 중 연결·tx를 점유하지 않는다. (확인)
- **조회 계약:** 유효 퍼미션 조회도 계정 존재를 일반 조회로 확인하고 미존재 시 `RbacNotFound`를 반환하도록 수정했다. 계정 역할 조회와 같은 404 계약을 따른다. (확인)
- **다음 작업 07:** `auth/mod.rs`의 local_account 선언과 input·준비·해싱·저장 함수 재노출은 리뷰 수정에서 완료해 원본 앱이 컴파일된다. `find_account` 재노출, `rbac/slice/mod.rs`의 06 다섯 slice 선언, HTTP·bootstrap CLI 등록과 가입 진입 인가는 남는다. 기존 Container의 DB/resolver를 재사용하며 정확한 API는 작업 문서 참조. (확인)
- **검증·리뷰 상태:** 404 수정 후 원본 workspace fmt·Clippy와 RBAC 실제 DB 테스트 26개가 모두 통과했다. 직전 원본 인증·가입 테스트 64개 통과 기록은 유지하며 이번 변경은 RBAC 조회에 한정된다. audit은 재실행해 같은 RUSTSEC-2026-0459 실패를 기록했다. 이번 주요 결함은 해소했으며 전체 판정은 R3 운영 복구 근거·실제 관리 HTTP/CLI 검증을 남겨 보류한다. 상세 결과는 작업 문서 하단 참조. 05 리뷰의 기존 보완 사항은 남으며 DB 검증은 고유 PostgreSQL 스키마만 사용한다. (확인·추론)
- **진행 기준:** 의미 구분 타입·공통 AppResult를 유지하고 생성 검증·임의 상한·별도 오류 추상화를 추가하지 않는다. 사용자 AGENTS.md 변경을 보존했으며 커밋·다른 에이전트 실행은 하지 않았다. (확인)

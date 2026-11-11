# 이어서 할 작업

- **작업 문서:** [RBAC 07/08 현재 구현·재개 기준](specs/rbac/manage-access/plan.md#rbac-07-08-current), [실행 검증](specs/rbac/manage-access/verification.md). (확인)
- **구현:** 01~08 연결·구현·검증·자체 점검을 완료했다. 원본 Container의 DB/resolver를 재사용한 JSON·웹 RBAC Router, 가입 account.create 진입 인가, stdin bootstrap-admin CLI를 연결했다. (확인)
- **현재 계약:** 조회는 현재 DB 역할의 일반 조회다. 부여/회수는 계정 잠금 없이 존재·상태를 확인하고 역할 정렬 잠금 후 같은 tx에서 재인가한다. 기존 JWT 역할은 만료까지 남고 현재 역할 퍼미션 변경은 다음 조회부터 적용된다. bootstrap은 단독 실행하며 현재 전체 카탈로그를 batch 저장한다. auth 공용 준비→연결 반환→해싱→저장 tx 계약을 유지한다. (확인)
- **검증:** 원본 앱·CLI·저장소의 격리 PostgreSQL 테스트, workspace 회귀와 라이브러리 DB 검사, fmt·Clippy 결과를 작업 문서에 기록했다. 05의 탈퇴 계정 거짓 양성을 보완하고 서로 다른 행위자의 실제 HTTP 역할 편집·관리 및 부여 경합을 검증했다. (확인)
- **남은 한계:** audit은 기존 RUSTSEC-2026-0459로 실패하며 무시 설정·의존성 변경 없이 기록했다. 운영 적용·DB 보정/롤백 리허설·부하·commit 중 통신 단절은 미검증이다. RBAC plan은 draft이며 별도 사용자 수락이나 배포는 하지 않았다. (확인)
- **작업 상태:** 커밋·배포·다른 에이전트 실행 없음. 개발/운영 업무 데이터는 변경하지 않았고 테스트 고유 스키마만 사용했다. 추가 작업은 새 사용자 요청에 따른다. (확인)

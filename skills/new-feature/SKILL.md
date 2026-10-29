---
name: new-feature
description: Orchestrate requirements, design, implementation, verification, and review for a new feature with resumable task-local records.
---

# 새 기능 워크플로

스킬 경로는 이 파일 기준이다. 각 단계에서 연결된 `SKILL.md`를 읽고 같은 작업 디렉터리를 전달한다.

## 시작과 재개

1. 기존 작업 디렉터리가 지정되면 재사용하고, 새 작업이면 [state-mgmt](../state-mgmt/SKILL.md)로 Git 루트 아래에 생성한다. 기존 작업이 불분명하면 임의로 선택하지 않는다.
2. `state-mgmt`로 `new_feature` 작업을 등록하거나 조회하고 `workflow.md`에 연결한다. 작업 중에는 `progress`로 기록한다. 기존 상태와 문서는 보존한다.
3. `workflow.md`가 없으면 아래 목록을 만들고 목표와 비교 기준 커밋을 기록한다. 기준 커밋을 확인할 수 없으면 이유를 기록한다. 재개 시 목록, 상태, 관련 문서와 실제 코드·커밋을 대조해 불일치를 해소하고 다음 행동부터 이어간다.
4. [task-log](../task-log/SKILL.md)로 최초 요청과 응답을 기록하고, 진행 중 주요 결정과 실제 반영한 사용자 정정을 누적한다. 요구사항이 바뀌면 관련 설계·작업·검증에 미치는 영향을 확인한다.

## 진행 목록

`workflow.md`에 다음 목록을 기록하고 각 항목에서 연결된 스킬을 적용한다. 결과와 다음 행동을 먼저 기록한 뒤 완료한 항목을 체크하고 상태를 갱신한다.

- [ ] 요구사항 정의 — [define-requirements](../define-requirements/SKILL.md) → `requirements.md`
- [ ] 초기 설계 — [create-design](../create-design/SKILL.md) → `design.md`
- [ ] 검증 절차 준비 — [simple-create-verification](../simple-create-verification/SKILL.md) → `verification/procedure.md`
- [ ] 작업 분할 — [breakdown-tasks](../breakdown-tasks/SKILL.md) → `tasks.md`
- [ ] 개별 Task 구현·검증·커밋 — [implement-task](../implement-task/SKILL.md) → 작업 문서와 커밋
- [ ] 전체 기능 검증 — [simple-create-verification](../simple-create-verification/SKILL.md) → `verification/result.md`
- [ ] 전체 변경 리뷰 — [review-work](../review-work/SKILL.md) → `review.md`
- [ ] 필요한 보완 및 재검증 — 관련 구현·검증·리뷰 절차; 보완이 불필요하면 이유 기록

검증 준비 시 미구현 기능의 실행은 구현 후로 남긴다. 각 Task는 실제 선행 결과를 확인하고 구현한다. 구현 중 확인된 변경이 설계나 검증 절차에 영향을 주면 관련 문서를 갱신한다. 리뷰에서 보완이 필요하면 해당 작업을 다시 진행하고 영향 범위를 재검증·재리뷰한다.

## 재개 정보와 완료

`workflow.md`에 현재 상황, 다음 행동, 미해결 사항과 관련 문서 경로를 간결히 유지한다. 체크 표시는 진행 요약이며 개별 상태는 `state.json`에서 확인한다. 중단 시 이유와 재개 조건을 기록하고 완료되지 않은 작업을 완료로 표시하지 않는다.

요구사항 범위의 구현, 필요한 검증과 전체 리뷰 및 필요한 보완을 마친 뒤 `new_feature`를 `complete`로 변경한다. 결과와 남은 한계, 커밋 및 문서 경로를 보고한다.

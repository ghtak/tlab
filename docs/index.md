# docs 구조

## 아키텍처 

프로젝트 아키텍처

## notes

정제되지 않은 생각, 아이디어, 조사 메모

## agent

참조 가이드, 지침, 스킬

기능별 스킬은 `.agents/skills/`에서 제공하며, 상세 절차는 아래 가이드 원문을 참조한다.
문서 관리 규칙은 별도 스킬로 만들지 않고 각 기능의 문서 작성·갱신에 공통으로 적용한다. (확인)

- [기능 구상 가이드](agent/feature-exploration/index.md) · [feature-exploration 스킬](../.agents/skills/feature-exploration/SKILL.md): 논의 대상의 기능·할 일·관계를 간략히 정리하고 상세 문서에 연결한다.
- [요구사항 정의 가이드](agent/requirements-definition/index.md) · [requirements-definition 스킬](../.agents/skills/requirements-definition/SKILL.md): 선택한 기능의 시나리오·정책·예외·검증 조건을 상세화한다.
- [설계 문서 작성 가이드 초안](agent/create-plan/index.md) · [create-plan 스킬](../.agents/skills/create-plan/SKILL.md)
- [작은 변경 처리 가이드](agent/small-change/index.md) · [small-change 스킬](../.agents/skills/small-change/SKILL.md)
- [커밋 가이드](agent/commit/index.md) · [commit 스킬](../.agents/skills/commit/SKILL.md)
- [코드 리뷰 가이드 초안](agent/code-review/index.md) · [code-review 스킬](../.agents/skills/code-review/SKILL.md)
- [앱 수준 검증 도구·방법 구성 가이드](agent/create-verification/index.md) · [create-verification 스킬](../.agents/skills/create-verification/SKILL.md)
- [문서 관리 규칙 초안](agent/document-management/index.md): 공통 참조 규칙

## specs

업무 작업


## verification

검증 작업

작업에서 재사용하는 기능 정의와 도구

- [공용 검증 도구](verification/tools/README.md)
- [로컬 DB 환경 Feature map](verification/feature/database-environment.md)

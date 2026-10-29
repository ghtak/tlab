# 상태 관리 스킬 생성 계획

## 목적과 범위

Git 저장소별 작업 디렉터리에 개별 작업의 상태와 관련 문서 경로를 기록하는 재사용 가능한 스킬을 만든다. 스킬과 스크립트는 작업 간 순서, 의존성, 다음 실행 대상을 결정하지 않는다. 플레이북이나 호출자가 실행 여부를 판단한다.

## 저장 구조

- 현재 작업 디렉터리에서 `git rev-parse --show-toplevel`로 Git 루트를 찾는다. 저장소 밖이거나 루트를 확인할 수 없으면 오류를 반환한다.
- 새 작업의 기본 경로는 `{git-root}/tasks/{yyyymmdd}-{title}`이다. 날짜는 로컬 날짜를 사용한다.
- 제목은 파일명으로 사용할 수 있도록 정규화한다. 정규화 후 비어 있으면 오류를 반환한다. 동일 경로가 존재할 때 새 작업은 `-2`, `-3`을 붙여 생성하고, 기존 작업은 명시적으로 지정해 재사용한다.
- `state.json`과 작업 관련 문서는 작업 디렉터리 하위에 둔다. 문서 경로는 작업 디렉터리 기준 상대 경로로 저장하고, 정규화 및 심볼릭 링크 해석 후 디렉터리 밖으로 벗어나는 경로를 거부한다.

```text
tasks/20261029-example/
├── state.json
├── implementation/result.md
└── verification/result.md
```

## 상태 모델

```json
{
  "tasks": {
    "implement-login": {
      "status": "progress",
      "document": "implementation/result.md",
      "note": "세션 만료 처리 수정 중"
    }
  }
}
```

- 작업 ID는 해당 작업 디렉터리 안에서 고유하다. 스킬 이름과 작업 ID를 동일시하지 않는다.
- 상태값은 `pending`, `progress`, `complete`로 제한한다. 등록 시 기본값은 `pending`이다.
- 상태 변경 순서는 강제하지 않는다. 작업 간 조건이나 의존성은 저장하거나 검사하지 않는다.
- `document`, `note`는 선택 사항이다. 실패나 중단은 우선 메모에 기록한다. 별도 실패 상태는 필요성이 확인되면 추가한다.
- 현재 상태를 관리한다. 실행 이력이나 감사 로그는 초기 범위에서 제외한다.

## 스킬 및 스크립트 설계

- `SKILL.md`에 적용 시점, Git 루트 및 작업 디렉터리 결정, 상태 모델, 문서 경로 규칙, 오류 처리, 스크립트 호출 예제를 간결히 작성한다.
- 반복되는 JSON 읽기·검증·갱신은 Python 표준 라이브러리 기반 `scripts/state.py`에 맡긴다. 출력은 요청한 정보 또는 간결한 결과로 제한한다.
- 초기 명령: `init`, `add`, `show`, `set`. `init`은 새 작업 생성 또는 명시된 기존 작업 초기화를 지원한다. `add`는 ID 중복 시 기존 데이터를 보존하고 오류를 반환한다. `show`는 전체 상태 또는 지정 작업을 조회한다. `set`은 지정 작업의 상태와 선택적 문서 경로·메모를 갱신한다.
- 아래 호출은 구현할 인터페이스의 예시이며, 실제 실행 가능한 명령은 구현 후 확인한다.

```bash
python scripts/state.py init --title fix-login
python scripts/state.py init --directory tasks/20261029-fix-login
python scripts/state.py add tasks/20261029-fix-login implement-login
python scripts/state.py set tasks/20261029-fix-login implement-login progress
python scripts/state.py show tasks/20261029-fix-login implement-login
python scripts/state.py set tasks/20261029-fix-login implement-login complete --document implementation/result.md
```

- `init`은 기존 `state.json`을 덮어쓰지 않는다. 잘못된 JSON, 잘못된 상태값, 존재하지 않는 작업 ID, 잘못된 경로는 수정 없이 명확한 오류와 비정상 종료 코드로 반환한다.
- 파일 갱신은 같은 디렉터리의 임시 파일에 쓴 뒤 교체해 중단 시 손상 위험을 줄인다. 동시 실행은 초기 범위에서 보장하지 않으며, 실제 필요가 생기면 잠금 또는 충돌 감지를 추가한다.
- 토큰 절약을 위해 스킬은 필요한 작업만 조회하고, 관련 문서만 읽는다. 절감량은 구현 후 실제 호출 기준으로 측정한다.

## 구현 및 검증

1. 설치 위치와 기존 스킬 구조를 확인하고 `state-mgmt` 스킬의 `SKILL.md`와 필요한 Python 스크립트를 작성한다.
2. 임시 Git 저장소에서 신규 생성, 기존 상태 보존, 중복 ID, 상태 갱신과 재조회, 문서 경로 검증, 손상된 JSON 및 Git 저장소 밖 실행을 확인한다.
3. 스킬 생성 도구의 `quick_validate.py`로 스킬 형식을 검사하고, 문서에 적힌 호출 예제가 실제 동작하는지 확인한다.
4. 순서나 다른 작업의 상태에 대한 제약이 들어가지 않았는지 확인한다.

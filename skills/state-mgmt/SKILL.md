---
name: state-mgmt
description: Manage independent task statuses and document paths in a Git repository's task directory.
---

# 상태 관리

1. Git 저장소 안에서 실행한다. 새 작업은 `init --title 제목`으로 만들고, 기존 작업은 해당 작업 디렉터리를 지정한다. 새 디렉터리는 Git 루트의 `tasks/YYYYMMDD-제목` 아래에 만들어진다.
2. `add`로 고유한 작업 ID를 등록하고, `set`으로 `pending`, `progress`, `complete` 중 현재 상태를 기록한다. 상태 변경 순서와 작업 간 의존성은 강제하지 않는다.
3. 관련 문서는 작업 디렉터리 하위에 작성하고, 필요하면 `set --document`로 작업 디렉터리 기준 상대 경로를 연결한다. 실패나 중단은 `--note`로 기록한다.
4. 이어서 작업할 때는 `show`로 필요한 작업의 상태를 조회하고 관련 문서만 읽는다. 오류가 발생하면 기존 데이터를 보존하고 원인을 확인한다.

스킬 디렉터리의 `scripts/state.py`를 호출한다. 아래 `<script>`는 해당 스크립트의 경로이며, 작업 디렉터리는 Git 루트 기준 또는 절대 경로로 지정한다.

```bash
python <script> init --title fix-login
python <script> init --directory tasks/20261029-fix-login
python <script> add tasks/20261029-fix-login implement-login
python <script> set tasks/20261029-fix-login implement-login progress
python <script> show tasks/20261029-fix-login implement-login
python <script> set tasks/20261029-fix-login implement-login complete --document implementation/result.md --note "검증 완료"
```

`init --title`은 이름이 겹치면 접미사를 붙여 새 디렉터리를 만든다. `init --directory`는 기존 디렉터리를 사용하며 기존 상태 파일을 덮어쓰지 않는다. 여러 프로세스가 같은 상태 파일을 동시에 수정하지 않도록 한다.

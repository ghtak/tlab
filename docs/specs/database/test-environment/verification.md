---
id: VER-DATABASE-TEST-ENVIRONMENT
status: accepted
---

# 로컬 DB 환경 구성 검증

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| verifies | [SPEC-DATABASE-ENVIRONMENT](../../../verification/feature/database-environment.md) | 시작·접속·데이터 조작·종료·초기화 |

## 범위·준비

사용자의 요청에 따라 개발자용 Compose·just 명령과 공용 검증 자산을 구성한다.
사용법은 [DB 도구](../../../verification/tools/database/README.md)를 따른다. tlab DB API·드라이버·CI·자동 실행별 격리는 이번 범위에 포함하지 않는다. (확인)

1. `just db-doctor`로 Docker 접근과 구성을 확인한다.
2. `tlab-db` 프로젝트의 기존 컨테이너·볼륨이 없는지 확인한다. 기존 데이터가 있으면 초기화 검증을 하지 않는다.
3. 이번 실행이 만든 환경에서만 아래 데이터 유지·초기화 검증을 수행한다.

## 수행·판정

1. `just db-up` → `just db-status`: 세 DB가 healthy인지 확인한다.
2. `just db-check`: 세 DB 각각의 쓰기·조회 성공 출력과 종료 코드 0을 확인한다.
3. 각 내부 클라이언트에서 아래 데이터를 저장한다.
   - PostgreSQL: `CREATE TABLE tlab_persistence_check (value integer); INSERT INTO tlab_persistence_check VALUES (42);`
   - Valkey: `SET tlab:persistence-check 42`
   - Oracle: `CREATE TABLE tlab_persistence_check (value NUMBER); INSERT INTO tlab_persistence_check VALUES (42); COMMIT;`
4. `just db-down` → `just db-up` 후 양쪽 SQL에서 `SELECT value FROM tlab_persistence_check;`, Valkey에서 `GET tlab:persistence-check`로 값 42의 유지를 확인한다.
5. `just db-reset` → `just db-up` 후 PostgreSQL은 `SELECT to_regclass('public.tlab_persistence_check');`가 NULL, Oracle은 `SELECT COUNT(*) FROM user_tables WHERE table_name = 'TLAB_PERSISTENCE_CHECK';`가 0, Valkey는 `EXISTS tlab:persistence-check`가 0인지 확인한다.
6. `just db-check`로 새 환경도 접속·조작 가능한지 확인한다.
7. `just db-reset`으로 이번 실행의 컨테이너·볼륨을 정리하고 부재를 확인한다.

실패 시 `just db-status`, `just db-logs <서비스>`로 근거를 확인한다. 각 단계를 실행했는지와 기대 결과 충족 여부를 구분한다.

## 검증 결과

2026-11-04, macOS ARM64의 Rancher Desktop에서 실행했다. Docker Engine 29.5.3, Compose v5.3.1, just 1.57.0을 사용했으며 세 이미지 모두 ARM64로 실행했다. (확인)

| 확인 항목 | 실제 수행·관찰 | 판정 |
| --- | --- | --- |
| 구성·구문 | Compose `config --quiet`, `just --fmt --check`, `sh -n`, Markdown 로컬 링크·공백 검사 통과 | 통과 |
| 준비 확인 | `just db-doctor`에서 Docker 버전 조회·Compose 구성 확인, 종료 코드 0 | 통과 |
| 전체 기동 | `just db-up` 후 세 DB 모두 `healthy`, 루프백에 5432·6379·1521 바인딩 | 통과 |
| 쓰기·조회 | `just db-check`에서 세 DB의 `read/write OK`, 종료 코드 0 | 통과 |
| Valkey 호스트 접속 | 호스트의 `127.0.0.1:6379`에 Python 표준 socket으로 RESP 요청, 임시 키 SET·GET·DEL 성공 | 통과 |
| 임시 데이터 정리 | PostgreSQL 임시 테이블 수 0, Valkey 검증 키 조회 결과 없음, Oracle private temporary table 수 0 | 통과 |
| 데이터 유지 | 세 DB에 42 저장 후 `db-down` → `db-up`, 세 DB 모두 조회값 42 | 통과 |
| 개별 기동 | 초기화 후 `db-up postgres valkey`는 두 서비스만 기동, 이어 `db-up oracle`도 정상 기동 | 통과 |
| 전체 초기화 | `db-reset` 후 재기동: PostgreSQL 기존 테이블 없음, Valkey 기존 키 수 0, Oracle 기존 테이블 수 0 | 통과 |
| 초기화 후 사용 | `just db-check` 재실행에서 세 DB 모두 성공 | 통과 |
| 상태·로그 | `db-status`로 상태 확인, `db-logs oracle`에서 준비 완료·앱 계정 생성 확인 | 통과 |
| 포트 변경 | `TLAB_POSTGRES_PORT=15432`의 Compose 구성 결과가 `127.0.0.1:15432`인지 확인. 변경 포트의 실제 기동은 미실행 | 구성 확인 |

사용한 이미지 식별자 앞 12자리: `postgres:18.6` → `07ee628d2e71`, `valkey/valkey:8.1.11` → `76ec93b657e3`, `gvenzl/oracle-free:23.26.3` → `4d18b635ea14`. (확인)

- Mac Intel·Windows·Docker Desktop: 미실행. Windows 셸 설정·LF 줄바꿈 규칙을 추가했지만 실제 OS 검증은 남아 있다.
- PostgreSQL·Oracle의 호스트 클라이언트 접속, Rust 드라이버·tlab API: 미검증, 이번 범위 밖.
- Rust 소스·의존성 변경이 없어 Cargo 테스트는 실행하지 않았다.

## 정리 결과

검증 시작 전 `tlab-db` 프로젝트의 컨테이너·볼륨이 없음을 확인했다.
유지·초기화 검증 후 `just --yes db-reset`을 실행하여 이번 실행의 컨테이너 3개·볼륨 3개·네트워크를 제거했다.
Docker의 Compose 프로젝트 라벨로 컨테이너·볼륨·네트워크를 조회한 결과 모두 없었다. 내려받은 이미지는 캐시에 유지했다. (확인)
이후 Valkey 단독 기동으로 호스트 접속을 추가 확인하고 다시 초기화했으며, 같은 라벨 조회로 자원이 남지 않았음을 재확인했다. (확인)

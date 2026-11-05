# 로컬 DB 환경

PostgreSQL·Valkey·Oracle Free를 개발자 PC에서 실행한다. 호스트에는 Docker와 Compose, just가 필요하며 DB 클라이언트는 컨테이너에 포함된 것을 사용한다.
프로젝트 루트의 `justfile`이 이 디렉터리의 Compose를 호출한다. (확인)

## 준비

- Mac 또는 Windows에서 Linux 컨테이너를 실행할 수 있는 Docker 환경과 `docker compose`, `just`를 준비한다.
- Rancher Desktop에서는 Docker CLI를 사용할 수 있는 `dockerd (moby)` 엔진을 선택한다.
- Windows의 just 명령은 기본 제공 PowerShell을 사용하도록 설정했다. WSL에서 실행할 때는 Linux 환경의 Docker CLI와 just를 사용한다.
- 이미지가 호스트의 아키텍처를 선택하도록 `platform`은 강제하지 않는다.
- 세 DB, 특히 Oracle의 이미지 다운로드·최초 초기화에는 시간이 걸린다. 준비 대기 한도는 600초다.

도구·이미지 근거: [Rancher Desktop](https://docs.rancherdesktop.io/ui/preferences/container-engine/general/), [just](https://just.systems/man/en/settings.html), [PostgreSQL](https://hub.docker.com/_/postgres), [Valkey 이미지](https://github.com/valkey-io/valkey-container), [Oracle Free 이미지](https://github.com/gvenzl/oci-oracle-free). 실제 실행 확인 범위는 작업별 검증 기록을 따른다. (확인)

## 실행·확인

프로젝트 루트에서 실행한다. 별도 설정 파일 복사는 필요 없다.

```sh
just db-doctor
just db-up
just db-status
just db-check
```

`db-doctor`는 Docker 접근과 Compose 설정만 확인한다. `db-up`은 healthcheck 완료를 기다린다.
`db-check`는 테스트 계정으로 접속하여 값을 쓰고 다시 조회하며, 값이 다르거나 명령이 실패하면 0이 아닌 종료 코드를 반환한다.
PostgreSQL·Oracle은 세션 전용 임시 테이블과 rollback을 사용한다. Valkey는 전용 임시 키를 만들고 삭제하며, 중단 시에도 60초 뒤 만료된다. (확인)

일부 DB만 필요하면 서비스를 지정하고 해당 확인 명령을 실행한다. `db-check`는 세 DB가 모두 실행 중일 때 사용한다.

```sh
just db-up postgres valkey
just db-check-postgres
just db-check-valkey
just db-check-oracle
just db-logs oracle
```

각 확인 명령은 `PostgreSQL read/write OK`, `Valkey read/write OK`, `Oracle read/write OK`를 출력한다.
시작 실패 시 `just db-status`와 `just db-logs <서비스>`로 상태·로그를 확인한다. `db-up`이 실패해도 생성된 컨테이너는 남을 수 있으며, 원인 확인 후 재실행하거나 `db-down`으로 정리한다. (확인)

## 접속

아래 계정은 저장소에 공개된 로컬 개발용 기본값이다. 호스트 포트는 `127.0.0.1`에만 연결한다. (확인)

| 서비스 | 호스트 접속 정보 | 계정 | 내부 클라이언트 |
| --- | --- | --- | --- |
| PostgreSQL 18.6 | `127.0.0.1:5432`, DB `tlab` | `tlab` / `tlab_dev` | `just db-client-postgres` |
| Valkey 8.1.11 | `127.0.0.1:6379`, DB `0` | 인증 없음 | `just db-client-valkey` |
| Oracle Free 23.26.3 | `127.0.0.1:1521/FREEPDB1` | `tlab` / `tlab_dev` | `just db-client-oracle` |

호스트 앱에서 사용할 PostgreSQL URL은 `postgres://tlab:tlab_dev@127.0.0.1:5432/tlab`, Valkey URL은 `redis://127.0.0.1:6379/0`이다.
Oracle 관리자 계정은 `system` / `tlab_admin_dev`이며, 개발·검증에는 `tlab` 계정을 사용한다. (확인)

이 확인 도구는 컨테이너 내부에서 TCP로 접속한다. 호스트 포트로의 접속, Rust 드라이버나 tlab API의 동작까지 검증하지는 않는다.
향후 DB 기능 작업에서는 이 환경을 준비 수단으로 재사용하고, tlab 공개 API를 호출하는 통합 테스트·실행 예제의 명령과 결과를 해당 작업의 `verification.md`에 연결한다. (확인·추론)

## 포트 변경

`defaults.env`를 명시적으로 사용하므로 루트 `.env`를 읽지 않는다. 포트가 충돌하면 같은 이름의 환경변수로 변경한다. 바꾼 포트는 호스트 접속 정보에도 반영한다. (확인)

Mac·WSL:

```sh
TLAB_POSTGRES_PORT=15432 just db-up postgres
```

Windows PowerShell:

```powershell
$env:TLAB_POSTGRES_PORT = '15432'
just db-up postgres
```

지원 변수는 `TLAB_POSTGRES_PORT`, `TLAB_VALKEY_PORT`, `TLAB_ORACLE_PORT`다.

## 데이터·정리

세 DB 모두 named volume에 데이터를 유지한다. Valkey는 AOF를 사용한다.
`just db-down` 후 다시 `just db-up`해도 데이터는 유지된다. (확인)

```sh
just db-down
```

깨끗한 DB가 필요하면 아래 명령을 사용한다. 확인 질문 후 `tlab-db`의 컨테이너와 세 볼륨을 삭제한다. 다시 시작하려면 `db-up`을 실행한다.

```sh
just db-reset
just db-up
```

이 환경은 고정 프로젝트명 `tlab-db`로 개발자가 직접 사용하는 하나의 환경이다. 같은 Docker 엔진의 여러 체크아웃도 같은 환경을 공유하므로 동시에 초기화·검증하지 않는다.
기존 데이터를 사용하는 환경에서는 `db-reset`을 실행하지 않는다. 자동 실행별 격리·CI 구성은 제공하지 않는다. (확인)

계정·DB 생성 설정은 빈 볼륨의 첫 초기화 때 적용된다. Compose의 초기 비밀번호를 수정해도 기존 계정의 비밀번호는 바뀌지 않는다.
DB 버전 변경, 특히 PostgreSQL 메이저 변경 시 기존 볼륨을 그대로 재사용할 수 있다고 가정하지 않는다. (확인)

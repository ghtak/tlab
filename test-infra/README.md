# 로컬 테스트 서비스

Docker Desktop의 Linux 컨테이너 환경에서 프로젝트 루트가 아닌 이 디렉터리에서 명령을 실행한다. PostgreSQL 17.11, Oracle Database Free 26ai Lite, Redis 7.4.12를 사용한다. Oracle Lite는 기능 제한이 있으므로 운영 환경과의 호환성은 별도로 확인한다.

```powershell
cd test-infra
docker compose up -d postgres redis
docker compose up -d oracle
docker compose ps
docker compose logs -f oracle
docker compose stop
```

필요한 서비스만 이름으로 지정해 실행한다. Oracle은 초기화가 오래 걸릴 수 있다. 로그의 준비 완료 표시를 확인한 뒤 실제 접속을 확인한다. PostgreSQL과 Redis는 `docker compose ps`에서 health 상태를 확인한다. 컨테이너가 실행 중인 것만으로 DB 접속 준비가 완료된 것은 아니다.

| 서비스 | 로컬 접속 | 기본 계정 |
| --- | --- | --- |
| PostgreSQL | `localhost:5432`, DB `test` | `test` / `localtest123` |
| Oracle | `localhost:1521/FREEPDB1` | `system` / `Localtest123` |
| Redis | `localhost:6379` | 인증 없음 |

기본 비밀번호는 로컬 테스트 전용이다. 포트나 비밀번호를 바꾸려면 `.env.example`을 `.env`로 복사해 수정한다. 포트는 localhost에만 게시한다. Compose 네트워크에 참여하는 다른 컨테이너에서는 서비스 이름과 컨테이너 포트를 사용한다.

PostgreSQL과 Oracle의 데이터는 named volume에 보존된다. 초기 비밀번호는 기존 볼륨의 DB 계정을 자동으로 변경하지 않는다. Redis에는 영속 볼륨을 지정하지 않았다. 서비스가 공통 인스턴스이므로 프로젝트별 DB·스키마·데이터를 격리하고, 테스트가 서로의 데이터를 삭제하지 않도록 한다.

컨테이너만 중지·삭제하려면 `docker compose down`을 사용한다. `docker compose down --volumes`는 **PostgreSQL과 Oracle의 테스트 데이터를 삭제**하므로 초기화가 필요할 때만 사용한다.

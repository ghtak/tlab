[windows]
set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

db-compose := "docker compose --env-file docs/verification/tools/database/defaults.env -f docs/verification/tools/database/compose.yaml"

default:
    @just --list

# Docker 접근과 Compose 설정 확인
db-doctor:
    docker version
    {{ db-compose }} config --quiet

# 전체 또는 지정한 DB 시작: just db-up postgres valkey
db-up *services:
    {{ db-compose }} up --wait --wait-timeout 600 {{ services }}

db-status:
    {{ db-compose }} ps -a

db-logs *services:
    {{ db-compose }} logs --tail 100 {{ services }}

# 세 DB의 테스트 계정 접속 및 쓰기·조회 확인
db-check: db-check-postgres db-check-valkey db-check-oracle

db-check-postgres:
    {{ db-compose }} exec -T -e PGPASSWORD=tlab_dev postgres psql -X -h 127.0.0.1 -U tlab -d tlab -v ON_ERROR_STOP=1 -f /verification/check.sql

db-check-valkey:
    {{ db-compose }} exec -T valkey sh /verification/check.sh

db-check-oracle:
    {{ db-compose }} exec -T oracle sqlplus -L -s tlab/tlab_dev@//localhost:1521/FREEPDB1 @/verification/check.sql

db-psql:
    {{ db-compose }} exec -e PGPASSWORD=tlab_dev postgres psql -X -h 127.0.0.1 -U tlab -d tlab

db-valkey:
    {{ db-compose }} exec valkey valkey-cli

db-sqlplus:
    {{ db-compose }} exec oracle sqlplus -L tlab/tlab_dev@//localhost:1521/FREEPDB1

# 컨테이너 제거, 데이터 유지
db-down:
    {{ db-compose }} down

# 로컬 DB 데이터 삭제. 재생성은 db-up
[confirm("tlab-db의 전체 DB 데이터를 삭제합니다. 계속할까요?")]
db-reset:
    {{ db-compose }} down --volumes

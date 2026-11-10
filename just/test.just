# workspace 기본 테스트 (DB 연동 등 ignored 테스트 제외)
test:
    cargo test --workspace --locked

# environment_scenario는 다른 테스트가 격리된 프로세스에서 실행하는 보조 항목
# PostgreSQL 연동을 포함한 전체 테스트 (DB 주소 지정 가능)
test-all database_url="postgres://tlab:tlab_dev@127.0.0.1:5432/tlab":
    TLAB_TEST_DATABASE_URL="{{ database_url }}" cargo test --workspace --locked -- --include-ignored --skip environment_scenario

# 격리된 자식 프로세스를 통한 설정 환경변수 시나리오 검증
test-config-env:
    cargo test -p tlab --locked --test config environment_

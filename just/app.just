# 앱을 빌드하고 포그라운드로 실행. 종료는 Ctrl+C
app-run:
    cargo run -p tlab-app --locked

# 앱 DB migration 실행. 필요하면 migration 디렉터리를 인자로 지정
app-migration directory="tlab-app/migrations":
    cargo run -p tlab-app --locked -- --migrate "{{ directory }}"

# 최초 관리자 추가. 기본 admin@example.com / passwd. 비밀번호 변경은 TLAB_BOOTSTRAP_PASSWORD로 지정
[unix]
app-bootstrap-admin email="admin@example.com" nickname="admin":
    @printf '%s\n' "${TLAB_BOOTSTRAP_PASSWORD:-passwd}" | cargo run -p tlab-app --locked -- bootstrap-admin --email {{ quote(email) }} --nickname {{ quote(nickname) }}

# 최초 관리자 추가. 기본 admin@example.com / passwd. 비밀번호 변경은 TLAB_BOOTSTRAP_PASSWORD로 지정
[windows]
app-bootstrap-admin email="admin@example.com" nickname="admin":
    @$bootstrapPassword = if ($env:TLAB_BOOTSTRAP_PASSWORD) { $env:TLAB_BOOTSTRAP_PASSWORD } else { 'passwd' }; $bootstrapPassword | cargo run -p tlab-app --locked -- bootstrap-admin --email {{ quote(email) }} --nickname {{ quote(nickname) }}

# UI 개발 서버를 포그라운드로 실행. 종료는 Ctrl+C
app-run-ui:
    npm --prefix tlab-app-ui run dev

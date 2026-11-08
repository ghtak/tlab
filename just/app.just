# 앱을 빌드하고 포그라운드로 실행. 종료는 Ctrl+C
app-run:
    cargo run -p tlab-app --locked

# 앱 DB migration 실행. 필요하면 migration 디렉터리를 인자로 지정
app-migration directory="tlab-app/migrations":
    cargo run -p tlab-app --locked -- --migrate "{{ directory }}"

# UI 개발 서버를 포그라운드로 실행. 종료는 Ctrl+C
app-run-ui:
    npm --prefix tlab-app-ui run dev

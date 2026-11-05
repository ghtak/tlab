# 앱을 빌드하고 포그라운드로 실행. 종료는 Ctrl+C
app-run:
    cargo run -p tlab-app --locked

# UI 개발 서버를 포그라운드로 실행. 종료는 Ctrl+C
app-run-ui:
    npm --prefix tlab-app-ui run dev

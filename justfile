[windows]
set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

[unix]
set shell := ["sh", "-cu"]

test-up:
    docker compose -f test-infra/compose.yaml up -d

test-down:
    docker compose -f test-infra/compose.yaml down

#!/bin/sh
set -eu

# NX로 다른 키를 덮어쓰지 않고, 중단되더라도 60초 후 만료시킨다.
key="tlab:verification:$(date +%s):$$"
test "$(valkey-cli --raw SET "$key" 42 NX EX 60)" = OK
trap 'valkey-cli DEL "$key" >/dev/null' EXIT
test "$(valkey-cli --raw GET "$key")" = 42
echo 'Valkey read/write OK'

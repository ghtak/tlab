curl-bin := if os() == "windows" { "curl.exe" } else { "curl" }

# 실행 중인 앱의 상태·본문 확인. HTTPS는 --cacert <파일> 지정 가능
http-health base_url="http://127.0.0.1:8080" *curl_args:
    {{ curl-bin }} --fail-with-body --silent --show-error --connect-timeout 2 --max-time 5 --write-out "\nHTTP %{http_code}\n" {{ curl_args }} "{{ base_url }}/health"

# HTTP 앱 실행·검증 도구

`just`를 공통 명령 진입점으로 사용하고 단순 요청은 `curl`로 관찰한다.
여러 요청의 상태·응답을 연결하거나 JSON·쿠키를 검증할 필요가 생기면 Python `requests` 시나리오를 추가한다.
이번 생존 확인에는 Python 의존성을 추가하지 않았다. (추론·선택)

근거: [just 명령 실행 모델](https://just.systems/man/en/), [curl 옵션](https://curl.se/docs/manpage.html), [Requests 응답·timeout 사용법](https://requests.readthedocs.io/en/latest/user/quickstart/).
기능별 기대 결과는 [생존 확인 Feature map](../../feature/http-health.md), 작업별 관찰 결과는 [VER-HTTP-APP](../../../specs/http/app/verification.md)을 따른다.

## 실행과 확인

Rust 도구 체인·just·curl을 준비하고 저장소 루트에서 실행한다.
기본 주소는 `http://127.0.0.1:8080`이다. DB를 가동할 필요는 없다.

```sh
just app-run
```

다른 터미널에서 요청한다.

```sh
just http-health
```

정상 응답은 본문 `ok` 다음 줄에 `HTTP 200`으로 표시된다. 프로세스 생존이나 기동 로그만으로 준비 완료라고 판단하지 않고 이 응답을 확인한다.
명령은 연결에 2초, 전체 요청에 5초 한도를 둔다. 연결 실패·4xx/5xx는 실패 코드로 전달한다.
명령 종료 코드 0만으로 정확한 상태·본문이 맞다고 판정하지 않고 출력값도 비교한다. 3xx는 자동으로 따라가지 않는다.
Windows에서는 `curl.exe`를 호출한다. (확인)

실제 상태·헤더·실패 경로를 직접 관찰하려면 다음 명령을 사용한다. Windows PowerShell에서는 `curl`을 `curl.exe`로 바꾼다.

```sh
curl --include --max-time 5 http://127.0.0.1:8080/health
curl --head --max-time 5 http://127.0.0.1:8080/health
curl --include --max-time 5 --request POST http://127.0.0.1:8080/health
curl --include --max-time 5 http://127.0.0.1:8080/heath
```

## 설정과 실행 격리

기본 파일은 `tlab-app/config/app.toml`이다. `TLAB_CONFIG_FILE`로 별도 파일을 선택하고 기존 `TLAB_APP_` 규칙으로 필드를 덮어쓸 수 있다.
예를 들어 포트를 바꾸면 요청 주소도 함께 바꾼다. (확인)

Mac·Linux:

```sh
TLAB_APP_SERVER__HTTP__BIND=127.0.0.1:18080 just app-run
```

Windows PowerShell:

```powershell
$env:TLAB_APP_SERVER__HTTP__BIND = '127.0.0.1:18080'
just app-run
```

다른 터미널에서:

```sh
just http-health http://127.0.0.1:18080
```

동시 검증은 별도 설정 파일·포트·로그 디렉터리를 사용한다. 상대 파일 경로는 프로세스의 작업 디렉터리 기준이다.
리스너 준비 로그에는 실제 바인딩 주소와 TLS 사용 여부가 표시되므로 포트 0으로 실행했다면 로그에서 할당 주소를 확인한다.

## HTTPS

`server.https`에는 `bind`와 필수 `tls` 설정을 둔다. HTTP 블록에는 TLS를 설정하지 않는다.
앱은 활성 인스턴스에 같은 Router를 전달한다. 서버 인증서에 맞는 호스트 이름과 신뢰 자료로 요청한다.

```sh
just http-health https://localhost:18443 --cacert /path/to/trusted-ca.pem
```

`--insecure`로 인증서 검증을 생략한 결과를 HTTPS 신뢰 검증으로 기록하지 않는다.
정적 파일은 `server.static_files`에 `directory`와 선택적 `prefix`를 지정하여 연결한다. 기본 접두사는 `/static`이다.

## 정리와 근거

실행 터미널의 Ctrl+C로 종료한다. Unix의 앱 프로세스에는 SIGTERM도 사용할 수 있다.
`just`와 Cargo도 Ctrl+C를 받으면 실행 래퍼는 130 등 중단 코드를 표시할 수 있으므로 앱의 종료 로그와 리스너 해제를 함께 확인한다.
종료 후 같은 주소로 다시 요청하여 연결 실패를 확인한다. 이번 실행의 프로세스·임시 설정·인증서만 정리하며 이름으로 모든 프로세스를 종료하지 않는다.

기본 로그는 `./logs`에 남는다. 임시 실행의 요청 결과·설정·로그 위치와 정리 상태는 작업별 `verification.md`에 기록한다.
Windows PowerShell에서 추가한 환경변수는 `Remove-Item Env:TLAB_APP_SERVER__HTTP__BIND` 등으로 현재 터미널에서 정리한다.

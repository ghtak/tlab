# 구글 로그인 연결 설정

구글 로그인에 사용할 Google Cloud 프로젝트와 OAuth 클라이언트를 준비하는 방법이다. 로그인 동작의 범위는 [구글 로그인 요구사항](req-google-login.md)을 따른다. (확인)

현재 앱에는 구글 로그인 경로와 해당 설정이 구현되어 있지 않다. 따라서 아래 절차로 Google 측 자격 증명을 준비할 수 있지만, 앱의 콜백 주소와 설정 항목이 결정되고 구현되기 전에는 실제 앱 로그인까지 완료할 수 없다. (확인)

## 준비할 정보

- Google Cloud 프로젝트를 생성하거나 설정할 권한이 있는 구글 계정.
- 사용자에게 표시할 앱 이름과 지원·개발자 연락처.
- 운영 서비스의 공개 주소와 개인정보처리방침 등 Google 콘솔이 요구하는 앱 정보.
- 구현에서 결정할 콜백 URI. 개발 환경과 운영 환경의 주소를 각각 준비한다. 아래 예시는 형식 설명이며 실제 앱의 경로가 아니다. (추론)

## Google Cloud 설정

1. [Google Cloud Console](https://console.cloud.google.com/)에서 프로젝트를 선택하거나 생성한다. 개발·테스트와 운영 프로젝트를 분리하는 것이 Google의 권장 사항이다. ([Google 검증 안내](https://support.google.com/cloud/answer/13461325?hl=en))
2. **Google Auth Platform → Overview**에서 시작하고, **Branding**에 앱 이름·지원 이메일·개발자 연락처와 요청되는 앱 정보를 입력한다. 사용자에게 표시되는 정보를 실제 서비스와 일치시킨다. ([Google Auth Platform 시작하기](https://support.google.com/cloud/answer/15544987?hl=en))
3. **Audience**에서 사용자 유형을 **External**로 선택한다. 모든 구글 계정을 대상으로 하려면 조직 구성원으로 제한하는 Internal을 사용하지 않는다. 테스트 상태에서는 등록한 테스트 사용자만 이용할 수 있으므로 **Test users**에 개발·검증용 구글 계정을 추가한다. 테스트 상태는 최대 100명으로 제한된다. ([대상 사용자 관리](https://support.google.com/cloud/answer/15549945?hl=en))
4. **Data Access**에서 로그인에 필요한 범위만 확인한다. 사용자 식별과 이메일·프로필 이름을 위한 `openid`, `email`, `profile`을 기준으로 하되, 실제 요청 범위는 로그인 구현에서 결정한다. 다른 Google API 권한은 필요할 때 별도로 추가한다. ([OpenID Connect](https://developers.google.com/identity/openid-connect/openid-connect))
5. **Clients → Create client**에서 **Web application**을 선택하고 이름을 지정한다. 서버 리디렉션 방식으로 구현하는 경우 **Authorized redirect URIs**에 앱이 실제로 처리할 콜백 URI를 등록한다. 브라우저에서 Google 라이브러리를 사용하는 방식이라면 구현에 필요한 **Authorized JavaScript origins**도 등록한다. 사용 방식과 실제 값은 후속 설계에서 결정한다. ([웹 서버 OAuth 설정](https://developers.google.com/identity/protocols/oauth2/web-server), [Google 로그인 설정](https://developers.google.com/identity/gsi/web/guides/get-google-api-clientid))
6. 생성된 **Client ID**와, 서버에서 사용하는 경우 **Client secret**을 안전하게 보관한다. 비밀 값은 생성 직후 안전한 비밀 저장소에 보관하고 저장소·브라우저 코드·로그·문서에 넣지 않는다. 개발과 운영은 각 환경의 자격 증명을 사용한다. ([웹 서버 OAuth 설정](https://developers.google.com/identity/protocols/oauth2/web-server))

### URI 작성 기준

- 예시: 운영 `https://app.example.com/<구현된-콜백-경로>`, 로컬 `http://localhost:포트/<구현된-콜백-경로>`. 실제 경로와 포트는 아직 정해지지 않았다. (추론)
- 등록 URI와 앱이 인증 요청에 보내는 URI는 스킴·호스트·포트·경로·대소문자·마지막 `/`까지 정확히 일치해야 한다. 불일치하면 `redirect_uri_mismatch`가 발생한다. 운영은 HTTPS를 사용하며 Google은 localhost에 HTTP 예외를 허용한다. ([웹 서버 OAuth 설정](https://developers.google.com/identity/protocols/oauth2/web-server))
- JavaScript origin은 페이지의 스킴·호스트·포트이며 경로를 포함하지 않는다. 콜백 URI와 목적이 다르므로 구현에 필요한 항목을 각각 등록한다. ([Google 로그인 설정](https://developers.google.com/identity/gsi/web/guides/get-google-api-clientid))

## 공개와 확인

1. 구현이 준비되면 개발 환경의 클라이언트와 등록된 테스트 사용자로 로그인·취소·재로그인을 확인한다. 최초 로그인 시 계정과 닉네임 생성, 재로그인 시 같은 계정 사용은 [요구사항](req-google-login.md)의 검증 조건을 따른다. (추론)
2. 전체 구글 계정에 공개하기 전 **Audience**에서 앱을 **In production**으로 게시한다. **Verification Center**에서 해당 앱에 필요한 검증과 제출 자료를 확인하고 Google의 안내를 따른다. 게시와 검증 상태는 구분하며, 요청 권한과 앱 정보에 따라 필요한 절차가 달라질 수 있다. ([게시·검증 안내](https://support.google.com/cloud/answer/13461325?hl=en), [Google Auth Platform 시작하기](https://support.google.com/cloud/answer/15544987?hl=en))
3. 운영 주소와 운영 클라이언트로 로그인하고, 테스트 사용자로 등록하지 않은 구글 계정도 사용할 수 있는지 확인한다. 구글의 조직 정책 등 개별 계정 측 제한은 별도 원인으로 확인한다. (추론)

## 문제 확인

| 현상 | 확인할 항목 |
| --- | --- |
| `redirect_uri_mismatch` | 선택한 클라이언트와 등록 URI, 앱이 보낸 URI의 전체 값 일치 여부 |
| 테스트 사용자가 아닌 계정의 접근 거부 | Audience의 게시 상태와 테스트 사용자 등록 여부 |
| `org_internal` | Audience가 External인지, 올바른 프로젝트·클라이언트를 사용하는지 |
| 확인되지 않은 앱 경고 또는 게시 제한 | Verification Center의 상태와 요청 범위·브랜딩 검증 안내 |

오류 확인 시 인가 코드, 토큰, Client secret을 공유하거나 로그에 남기지 않는다. (추론)

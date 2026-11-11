# tlab-app-ui

`tlab-app-ui`는 tlab의 프런트엔드다. Rust 서버인 `tlab-app`과 별도로 개발 서버를 실행한다.

기술 구성·역할·코드 배치 기준은 [프런트엔드 개발 구조](../docs/architecture/fe-structure.md)에서 관리한다.

## 시작하기

Node.js 22.12 이상, npm, 프로젝트 루트의 `just` 명령이 필요하다. 프로젝트 루트에서 최초 한 번 의존성을 설치한다.

```sh
npm --prefix tlab-app-ui ci
```

로컬 DB 설정·migration·최초 관리자를 준비한 뒤, 별도 터미널에서 웹 인증용 백엔드와 UI를 실행한다. 기존 `TLAB_APP_DATABASE__URL` 또는 루트 `.env`의 DB 설정을 사용한다. migration과 관리자 추가는 새 개발 DB에 필요한 경우에만 실행한다.

```sh
just app-migration
just app-bootstrap-admin
just app-run
```

다른 터미널에서 UI를 실행하고 `http://localhost:5173`으로 접속한다. Ctrl+C로 종료한다. 저장소의 `tlab-app/config/app.toml`은 이 주소에 맞는 로컬 인증 설정을 제공한다. 설정을 변경했다면 기존 백엔드를 종료한 뒤 `just app-run`으로 다시 실행해야 반영된다. `localhost` 대신 `127.0.0.1`로 UI에 접속하면 다른 origin이므로 안내한 주소를 사용한다. 배포 시에는 `auth.web.origin`을 외부 HTTPS 주소로, `allow_insecure_http`를 `false`로 지정한다.

```sh
just app-run-ui
```

`tlab-app-ui` 디렉터리에서 직접 `npm run dev`를 실행할 수도 있다. 의존성 버전은 `package-lock.json`으로 관리한다. UI 포트는 5173으로 고정된다. 별도 검증 서버가 필요하면 `TLAB_UI_API_TARGET`으로 proxy 대상을 지정하고 Vite의 `--port` 옵션 및 백엔드 `auth.web.origin`도 같은 UI 주소로 맞춘다.

## 명령

아래 명령은 `tlab-app-ui` 디렉터리에서 실행한다.

| 명령 | 동작 |
| --- | --- |
| `npm ci` | 잠금 파일에 따라 의존성을 설치한다. |
| `npm run dev` | Vite 개발 서버를 실행한다. |
| `npm run build` | 프로덕션 파일을 `dist/`에 생성하고 TypeScript 타입을 검사한다. |
| `npm run preview` | 빌드된 결과를 로컬에서 확인한다. 먼저 빌드가 필요하다. |

## 테마

화면 상단에서 라이트·다크·시스템 설정을 선택한다. 기본값은 시스템 설정이며, 선택은 이 브라우저의 `localStorage` (`tlab.theme`)에 저장된다. 시스템 설정을 선택하면 OS 색상 모드 변경도 즉시 반영한다. 저장소를 사용할 수 없으면 현재 페이지에서만 선택을 유지한다.

- `src/index.css`: Tailwind와 공통 스타일을 가져오는 진입점이다.
- `src/styles/theme.css`: `:root`와 `.dark`가 테마 값을 정의하고 `@theme inline`이 Tailwind 유틸리티에 연결한다. 화면은 `bg-background`, `bg-card`, `text-foreground`, `text-destructive`, `border-border`처럼 의미 기반 토큰을 사용한다.
- `src/styles/base.css`: body의 기본 글꼴·배경·글자색 등 전역 기본 스타일을 정의한다.
- `src/components/theme-provider.tsx`: 앱 공통 테마 상태와 시스템 색상 모드 구독을 소유한다. 인증·서버 상태와 독립적으로 `main.tsx`에서 조립한다.
- `src/components/theme-select.tsx`: 공통 선택 UI다. 다른 공통 UI에서도 `useTheme()`으로 선택을 읽거나 변경할 수 있다.
- `index.html`: React가 실행되기 전에 저장된 선택을 적용해 초기 밝은 화면의 깜빡임을 방지한다. 저장 키·선택 값을 변경하면 Provider와 함께 맞춘다.

## 검증 실행

```sh
npm test
npx playwright install chromium
npm run test:browser
```

브라우저 검증은 실행 중인 UI가 필요하다. 기본 실행은 mock 오류 시나리오를 확인하고 실제 BE 테스트는 건너뛴다. 실제 연동은 **격리 DB의 테스트 계정**을 `{ "email": "...", "password": "..." }` 형태의 저장소 밖 접근 제한 JSON 파일에 준비하고 `TLAB_UI_TEST_CREDENTIALS`로 경로를 전달한다. `TLAB_UI_BASE_URL`로 별도 UI 주소를 지정할 수 있다. 정상 만료 Access의 갱신 검증은 해당 격리 서버의 키 파일 경로를 `TLAB_UI_TEST_SIGNING_KEY`로 전달했을 때 추가 수행한다. 운영 키를 사용하지 않는다. 비밀번호·쿠키를 포함할 수 있는 trace·스크린샷·영상은 저장하지 않는다.

구현·검증 범위와 세션 계약은 [인증 UI 설계·구현 기록](../docs/specs/auth/web-ui-session/plan.md)을 참조한다.

테마 변경 리뷰: `bf131b1` 대비 미커밋 변경을 검토했으며 결함은 발견하지 못했다. `npm test` 8개, `npm run build`, `npm run test:browser -- tests/theme.spec.ts` 3개, `git diff --check`가 통과했다. 의존성·잠금 파일이 같아 앞선 `npm audit --json`의 취약점 0건 결과를 재사용했다. Rust 변경은 없어 Rust 검사는 비대상이다. (확인)

판정은 통과, 품질 점수는 기능 3·구조 4·안정성 3·성능 4·테스트 용이성 4 (각 4점), 보안 N/A로 85.94/100이다. 리스크 R1: 저장된 테마 값은 이전 코드가 사용하지 않아 코드 롤백으로 기존 화면을 복원할 수 있으며, 실행 아키텍처 변경은 없다. 실제 롤백과 Chromium 외 브라우저는 검증하지 않았다. (추론)

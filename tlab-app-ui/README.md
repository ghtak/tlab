# tlab-app-ui

`tlab-app-ui`는 tlab의 프런트엔드다. Rust 서버인 `tlab-app`과 별도로 개발 서버를 실행한다.

기술 구성·역할·코드 배치 기준은 [프런트엔드 개발 구조](../docs/architecture/fe-structure.md)에서 관리한다.

## 시작하기

Node.js 22.12 이상, npm, 프로젝트 루트의 `just` 명령이 필요하다. 프로젝트 루트에서 최초 한 번 의존성을 설치한다.

```sh
npm --prefix tlab-app-ui ci
```

개발 서버를 실행하고 터미널에 표시된 주소로 접속한다. Ctrl+C로 종료한다.

```sh
just app-run-ui
```

`tlab-app-ui` 디렉터리에서 직접 `npm run dev`를 실행할 수도 있다. 의존성 버전은 `package-lock.json`으로 관리한다.

## 명령

아래 명령은 `tlab-app-ui` 디렉터리에서 실행한다.

| 명령 | 동작 |
| --- | --- |
| `npm ci` | 잠금 파일에 따라 의존성을 설치한다. |
| `npm run dev` | Vite 개발 서버를 실행한다. |
| `npm run build` | 프로덕션 파일을 `dist/`에 생성하고 TypeScript 타입을 검사한다. |
| `npm run preview` | 빌드된 결과를 로컬에서 확인한다. 먼저 빌드가 필요하다. |

# tlab-app-ui

`tlab-app-ui`는 React로 만든 tlab의 프런트엔드다. 현재 기본 화면과 UI 개발을 위한 기반 구성이 있으며, Rust 서버인 `tlab-app`과는 별도로 실행된다. API 연동은 아직 구성하지 않았다.

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

## 기술 구성

| 기술 | 이 프로젝트에서의 역할 |
| --- | --- |
| [React](https://react.dev/) | 컴포넌트와 화면을 렌더링한다. |
| [TypeScript](https://www.typescriptlang.org/) | 컴포넌트와 설정의 타입을 검사한다. `tsconfig.json`에서 strict 모드와 `@/*` 경로를 설정한다. |
| [Vite](https://vite.dev/) | 개발 서버와 프로덕션 빌드를 제공한다. `vite.config.ts`에서 React, TanStack Router, Tailwind CSS 플러그인과 `@` 경로를 설정한다. |
| [TanStack Router](https://tanstack.com/router/latest) | `src/routes`의 파일을 기반으로 라우트를 구성한다. 플러그인이 `src/routeTree.gen.ts`를 생성하고 라우트 코드 분할을 처리한다. |
| [TanStack Query](https://tanstack.com/query/latest) | 비동기 데이터 조회와 캐시를 위한 기반이다. `src/main.tsx`에 `QueryClientProvider`가 설정되어 있으며 아직 조회는 구현되지 않았다. |
| [Tailwind CSS](https://tailwindcss.com/) | 유틸리티 클래스로 화면을 스타일링한다. `src/index.css`에 테마 색상을 정의한다. |
| [shadcn/ui](https://ui.shadcn.com/) | 프로젝트에 소스 코드를 두고 사용하는 UI 컴포넌트 구성이다. `components.json`에 경로와 스타일을 설정했고, `src/components/ui/button.tsx`에 버튼이 있다. |

버튼은 Radix Slot의 `asChild`, class-variance-authority의 변형 설정을 사용한다. `src/lib/utils.ts`의 `cn`은 clsx와 tailwind-merge로 조건부 클래스와 Tailwind 클래스를 합친다. Lucide 아이콘 라이브러리도 의존성에 포함되어 있다.

## 주요 파일

```text
tlab-app-ui/
├── components.json       shadcn/ui 설정
├── index.html            브라우저 진입 HTML
├── package.json          의존성과 npm 명령
├── vite.config.ts        Vite 플러그인과 경로 별칭
└── src/
    ├── main.tsx          React, Query, Router 초기화
    ├── index.css         Tailwind CSS 및 테마 색상
    ├── routeTree.gen.ts  라우터가 생성하는 파일
    ├── routes/           루트 레이아웃과 페이지 라우트
    ├── components/ui/    프로젝트에서 관리하는 UI 컴포넌트
    └── lib/utils.ts      공통 스타일 유틸리티
```

새 페이지는 `src/routes`에 추가한다. `__root.tsx`는 공통 루트, `index.tsx`는 `/` 경로를 담당한다. `routeTree.gen.ts`는 개발 서버나 빌드 시 생성되므로 직접 수정하지 않는다.

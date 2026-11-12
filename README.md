# tlab

Rust 공통 기반 라이브러리와 이를 사용하는 서버·웹 UI로 구성된 프로젝트다.
설정, 에러 처리, DB 접근, HTTP, 인증 등에 필요한 기반을 제공하고 앱에서 조합한다.

## 주요 구성

| 구성 | 역할 |
| --- | --- |
| [`tlab`](tlab/) | 여러 앱에서 재사용할 수 있는 Rust 기반 기능을 제공한다. |
| [`tlab-app`](tlab-app/) | 공통 기능과 업무 기능을 조합하고 서버·관리 CLI를 실행한다. |
| [`tlab-app-ui`](tlab-app-ui/) | 서버와 연결되는 React 웹 UI를 제공한다. |

## 문서 안내

- [아키텍처](docs/architecture/index.md): 구성요소의 책임·의존 관계와 개발 가이드.
- [개발 실행·검증](docs/verification/tools/README.md): 로컬 DB 환경과 서버 실행·확인 방법.
- [UI 개발](tlab-app-ui/README.md): 프런트엔드 준비·실행·빌드·검증 방법.
- [기능별 명세·설계](docs/specs/): 기능의 요구사항·계약·구현 및 검증 기록.
- [문서 안내](docs/index.md): 문서 영역별 용도와 작업 가이드.

개발 명령은 프로젝트 루트의 `justfile`에서 제공하며, `just --list`로 확인할 수 있다.

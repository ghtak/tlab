# 아키텍처

## 프로젝트 역할과 범위

워크스페이스는 공통 라이브러리 [`tlab`](../../tlab/Cargo.toml)과 실행 애플리케이션 [`tlab-app`](../../tlab-app/Cargo.toml)으로 구성된다. (확인)

- `tlab`은 에러 처리, 설정 로딩, DB, tracing, 키파일 관리 등 재사용 가능한 기반 기능을 제공한다.
- `tlab-app`은 `tlab`의 기능을 조합하고 실행 환경을 구성한다.
- 의존 방향은 `tlab-app → tlab`이며, `tlab`은 앱에 의존하지 않는다.
- 공통 기능은 `tlab`이 제공하고, 설정 선택과 초기화·종료 생명주기는 앱이 관리한다. (확인)

이 개요에는 역할·책임·의존 방향을 정리한다.
상세 계약과 결정 근거는 필요할 때 기능별 문서로 구성하고 여기에서 참조한다. (확인)

## 현재 구현 상태

- [에러 모듈](../../tlab/src/error.rs): 공통 에러 타입과 변환을 제공한다. 상세는 [에러 설계](../specs/error/plan.md)를 참조한다.
- [설정 모듈](../../tlab/src/config.rs): 파일과 환경변수를 병합해 호출자가 정의한 설정 타입으로 읽는다. 상세는 [설정 로딩 명세](../specs/config/load/spec.md)를 참조한다.
- [DB 모듈](../../tlab/src/sqlxdb.rs): SQLx 기반의 DB 접근과 트랜잭션 관리를 제공한다. 상세는 [DB 설계](../specs/database/plan.md)를 참조한다.
- [tracing 모듈](../../tlab/src/tracing.rs): 콘솔·파일 출력과 프로세스 전역 tracing 초기화를 제공한다.
- [키파일 모듈](../../tlab/src/keyfiles.rs): 소비자가 포함하는 파일 설정 타입과 TLS·Ed25519 자료의 로드·개발용 생성을 제공한다. 상세는 [키파일 관리 명세](../specs/keyfiles/spec.md)를 참조한다.
- [HTTP 모듈](../../tlab/src/http.rs): 앱이 조립한 Axum Router로 단일 HTTP 또는 HTTPS 리스너를 실행하고 정적 파일 Router 생성과 TLS 재로드를 제공한다. 다중 인스턴스 조정은 앱이 맡는다. [설계](../specs/http/plan.md)와 [검증·리뷰](../specs/http/verification.md)를 참조한다.
- [앱 진입점](../../tlab-app/src/main.rs): 설정·tracing 초기화, `/health`·정적 Router 조립, HTTP·HTTPS 인스턴스 실행·종료를 연결한다. [앱 HTTP 명세](../specs/http/app/spec.md)를 참조한다. (확인)

## 개발 가이드

설계·구현·테스트 시 참조하는 가이드를 정리한다.
`create-plan`으로 설계 문서를 작성할 때도 아래 가이드를 읽고 구현 방향과 검증할 계약·기대 결과에 반영한다. (확인)

- [구현 가이드](guide-impl.md): 기존 구조와 동작을 존중하며 필요한 범위만 구현·수정하고 실행으로 검증하는 원칙.
- [단위 테스트 가이드](guide-uint-test.md): 요구사항과 계약에 따른 기대 결과, 테스트 재사용, 변경과 위험에 맞는 검증 원칙. (확인)

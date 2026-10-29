# tlab 오류 처리 설계

## 입력과 현재 상태

- 입력: 같은 작업의 `requirements.md`와 사용자 대화.
- 확인됨: `tlab/src/lib.rs`에는 `add`와 단위 테스트만 있고, `tlab/Cargo.toml`에 의존성이 없다. 재사용할 오류 처리 코드는 없다.
- 합의됨: `error.rs`, `tlab::Result<T>`, `thiserror`, `Internal(anyhow::Error)`, `InvalidParameter(Cow<..>)`를 사용한다.

## 최소 설계안

사용자가 확인한 공개 계약이다. 아래 코드는 구현 시 컴파일과 원인 체인을 검증한다.

```rust
// tlab/src/error.rs
use std::borrow::Cow;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid parameter: {0}")]
    InvalidParameter(Cow<'static, str>),
    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`lib.rs`에서 오류 모듈을 선언하고 `Error`, `Result`를 크레이트 루트에 재공개한다. 모듈 자체는 비공개로 유지한다. `tlab/Cargo.toml`에 `thiserror`와 `anyhow`를 추가한다.

## 공개 계약과 경계

- `tlab::Result<T>`는 성공 시 `T`, 실패 시 `tlab::Error`를 반환하는 별칭이다.
- 호출자는 공개된 `Error` 변형을 패턴 매칭할 수 있다. 이는 향후 변형 추가 시 호출자의 완전 매칭 코드에 영향을 줄 수 있다.
- `InvalidParameter`는 정적 문자열 또는 소유한 `String`을 `Cow<'static, str>`로 받는다. 임시로 빌린 문자열은 소유 문자열로 변환해야 한다. 표시는 `invalid parameter: `와 전달된 설명이다.
- `Internal`은 `anyhow::Error`를 보유하고 `From<anyhow::Error>`를 제공한다. 표시는 `internal error: ` 접두사와 내부 오류 메시지다. 임의의 모든 오류 타입에서 `tlab::Error`로 직접 변환되는 것은 아니며, 필요할 때 먼저 `anyhow::Error`로 변환한다.
- 오류 타입과 별칭은 상태나 파일을 변경하지 않는다. 기존 `add`의 시그니처와 동작은 유지한다.

## 근거와 영향

- `Cow<'static, str>`는 정적 문자열을 빌리거나 동적으로 만든 메시지를 소유할 수 있고, 오류 타입에 수명 매개변수를 추가하지 않는다. 사용자가 이 수명 제한에 동의했다. 더 짧은 수명의 빌린 문자열은 그대로 담을 수 없다.
- 내부 메시지가 호출자에게 보이므로 민감한 정보를 담지 않도록 호출 지점에서 판단해야 한다. 원인 체인 전달은 구현 시 확인한다.
- 변경 범위는 `tlab/Cargo.toml`, 의존성 잠금 파일, `tlab/src/error.rs`, `tlab/src/lib.rs` 및 필요한 테스트다. 현재 저장소에서 확인된 다른 Rust 호출자는 없다.

## 검증 계획

- 외부 사용 관점의 테스트에서 `tlab::Result<T>`와 두 오류 변형을 사용한다.
- 정적 문자열과 소유 문자열을 `InvalidParameter`에 담고 표시 결과를 확인한다.
- `anyhow::Error`에서 `?`로 `Internal`에 변환되는 것과 접두사, 내부 메시지 및 원인 체인 전달을 확인한다.
- 기존 테스트를 포함해 `cargo test`를 실행한다. 이는 계획이며 아직 수행하지 않았다.

## 결정 사항

- 사용자가 `tlab::Error` 공개와 변형 직접 매칭, `Internal`의 `internal error: ` 접두사, `Cow<'static, str>`를 승인했다.
- 설계를 좌우하는 미확정 사항은 없다. 의존성 버전 선택과 코드의 컴파일 가능성은 구현 시 확인한다.

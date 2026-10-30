# tlab 설정 Loader 설계

상태: `complete` — 사용자가 설계 완료를 명시했다. 아래 설계안을 확정하며, 설계 완료는 구현·실행 검증 완료를 의미하지 않는다.-`측정함`

## 입력과 확인한 현재 상태

- 입력은 이 작업의 [requirements.md](requirements.md), 사용자가 제시한 Loader 코드·테스트와 후속 답변이다. 환경변수 단독 공개 사용, 환경변수 우선, 테스트 전용 역직렬화 구조체, `config.rs` 배치는 사용자가 확정했다.-`측정함`
- `tlab/src/lib.rs`는 기존 `Error`·`Result`를 재공개하며 `add`를 제공한다. `error.rs`의 `Internal(anyhow::Error)`와 `tlab/tests/error.rs`의 원인 체인 검증을 재사용할 수 있다.-`측정함`
- 현재 `tlab` 소스에는 Loader·환경변수 테스트 도우미가 없고, 기존 Loader 호출자도 검색되지 않았다. Cargo 의존성에는 `anyhow`, `thiserror`만 있다.-`측정함`
- 로컬 도구는 `rustc 1.96.0`, `cargo 1.96.0`이다. 실행 시 사용자 경로 canonicalize 경고가 출력되었으며, 실제 의존성 해석·빌드는 아직 수행하지 않았다.-`측정함`

## 최소 설계안과 호출 방식

설계 결정: `tlab/src/config.rs`에 `Loader` 하나를 두고, 파일용 `from_file`과 환경변수용 `from_env`로 생성한다. 파일과 환경변수를 함께 사용할 때는 기존 `env_prefix`를 연결한다. 제시 코드의 두 비공개 필드 `Option<PathBuf>`, `Option<String>`을 유지한다. 공개 경로는 `tlab::config::Loader`로 한다. 이는 사용자 확정 범위를 충족하면서 기존 예제를 유지하는 최소 변경이다.-`추론함`

```rust
use tlab::config::Loader;

// 파일만 읽기
let config = Loader::from_file("settings.toml".into()).load()?;

// 환경변수만 읽기
let config = Loader::from_env("APP").load()?;

// 파일을 읽은 뒤 환경변수로 덮어쓰기
let settings: Settings = Loader::from_file("settings.yaml".into())
    .env_prefix("APP")
    .try_deserialize()?;
```

위 코드는 호출 계약 예시이며, `Settings`는 호출자가 정의하는 `DeserializeOwned` 타입이다. 이 예제를 컴파일·실행한 것은 아니다.-`측정함`

처리 흐름: 생성자에서 입력을 보관 → `load`에서 파일 소스를 먼저 추가 → 환경변수 소스를 나중에 추가 → 한 번 `build` → `Config` 반환. `try_deserialize`는 이 `load`를 재사용한 뒤 `T`로 변환한다. 별도 파서나 설정 저장소는 두지 않는다.-`추론함`

## 공개 인터페이스 계약

아래 시그니처와 세부 동작은 요구사항을 구체화한 설계 계약이다. 사용자가 설계안의 완료를 요청하여 확정했으며, 각 세부 항목을 개별 질의·응답으로 확정한 것은 아니다.-`측정함`

| 인터페이스 | 입력과 상태 변화 | 출력·오류 | 부수 효과 |
| --- | --- | --- | --- |
| `Loader::from_file(path: PathBuf) -> Self` | 파일 경로를 소유하고 환경변수 소스는 비활성화한다. 상대 경로는 로딩 시 프로세스 작업 디렉터리를 기준으로 한다. | Loader를 반환한다. 파일 존재 여부는 여기서 검사하지 않는다. | 파일 I/O 없음 |
| `Loader::from_env(prefix: impl Into<String>) -> Self` | 접두사를 소유하고 파일 소스는 비활성화한다. | Loader를 반환한다. | 환경 읽기·쓰기 없음 |
| `env_prefix(self, prefix: impl Into<String>) -> Self` | 기존 파일 구성을 유지하고 접두사를 설정한다. 반복 호출 시 마지막 접두사로 교체한다. | 변경한 Loader를 반환한다. | 환경 읽기·쓰기 없음 |
| `load(self) -> crate::Result<::config::Config>` | Loader를 소비하고 선택된 소스를 읽는다. 생성 후 환경이 달라지면 로딩 시점 값을 읽는다. | 성공 시 독립된 Config, 읽기·파싱·병합 실패 시 `Error::Internal` | 동기 파일·환경 읽기. 파일·환경·전역 상태 쓰기 없음 |
| `try_deserialize<T: DeserializeOwned>(self) -> crate::Result<T>` | Loader를 소비하고 `load` 결과를 호출자 타입으로 변환한다. | 성공 시 T. 로딩 오류는 그대로 전파하며 변환 오류에는 `failed to deserialize config` 문맥을 붙여 `Error::Internal`로 반환한다. | `load`와 같은 읽기만 수행 |

소유권을 소비하는 방식은 사용자 제시 코드에 따른다. Loader 재사용·복제·소스 제거 API는 추가하지 않는다. `Config`를 별도 래퍼로 감싸거나 `tlab` 루트에 Loader를 재공개하지 않는다.-`추론함`

`lib.rs`에는 `pub mod config;`를 추가한다. 내부에서는 외부 크레이트를 `::config::Config`, `::config::File`, `::config::Environment`로 명시하여 같은 이름의 자체 모듈과 구분한다.-`추론함`

## 소스 처리·실패 경계

### 파일

설계 변경(rev-03): 사용자가 `config` 위임으로 되돌리도록 요청했다. `config::File::from(PathBuf)`를 필수 소스로 추가하고 파일 탐색·선택·읽기·파싱을 의존성에 맡긴다. 지정 경로가 없어도 의존성 규칙에 따라 후보 파일을 선택할 수 있으며, 설정 소스를 찾지 못하면 `Error::Internal`로 반환한다. Loader는 별도 경로 존재 검사·확장자 판별·BOM 제거·UTF-8 디코딩을 구현하지 않는다.-`측정함`

선택한 파일을 읽을 수 없거나 파싱할 수 없으면 로딩 전체를 실패시키며, 환경변수가 충분해도 무시하거나 환경변수만으로 재시도하지 않는다. 파일을 생성·수정하지 않는다. rev-02의 정확한 경로 읽기 방침은 R2 해석 재검토로 철회했으며 당시 구현·검증 이력은 개별 구현 결과에 보존한다.-`측정함`

### 환경변수와 우선순위

- 환경 소스는 `Environment::with_prefix(&prefix).prefix_separator("_").separator("__").try_parsing(true)`로 구성한다. 지정하지 않으면 환경 소스를 추가하지 않는다.-`추론함`
- 동일 키에서 환경변수가 파일보다 우선하는 것은 사용자 확정 계약이다. 파일에만 있는 값은 유지한다. 독자적인 병합 코드는 작성하지 않는다.-`측정함` / 병합 위임의 선택 근거-`추론함`
- 확인한 `config` 구현은 환경 키를 소문자로 바꾸고 접두사와 `_`를 제거한 뒤 `__`를 `.`으로 바꾼다. 값은 bool → i64 → f64 순서로 시도하고 해당하지 않으면 문자열로 둔다. 따라서 `APP_SERVER__PORT=8080`, `APP_SERVER__ENABLED=true`는 중첩된 숫자·불리언 값으로 읽는다. [config 환경 소스](https://docs.rs/config/0.15.27/src/config/env.rs.html)-`측정함`
- 설계 결정: 빈 값 무시, 목록 파싱, 임의의 접두사 정규화·trim·대문자 강제는 추가하지 않고 의존성의 동작을 유지한다. 빈 접두사도 새 오류로 거부하지 않는다. 확인한 구현에서는 빈 접두사와 `_`가 결합해 `_`로 시작하는 키를 대상으로 한다. 매칭 변수가 없어도 `load` 자체는 실패시키지 않으며 필수 필드 누락은 역직렬화에서 처리한다. [config 환경 소스](https://docs.rs/config/0.15.27/src/config/env.rs.html)-`추론함`

### 오류

설계 결정: 빌드 오류는 `Error::Internal(error.into())`로 감싸고 원인 오류를 유지한다. 역직렬화 오류만 제시 코드와 같은 `anyhow::Context` 문맥을 붙인다. 새 오류 변형·오류 로그·panic은 도입하지 않는다. 호출자의 타입 검증은 Serde와 호출자 타입에 맡기며, Loader가 도메인별 필수값이나 기본값을 따로 결정하지 않는다.-`추론함`

## 의존성과 선택 근거

확정한 설계의 Cargo 구성은 다음과 같다. 현재 Cargo 파일에는 적용하지 않았다.-`측정함`

```toml
[dependencies]
# 기존 anyhow, thiserror 유지
config = { version = "0.15.27", default-features = false, features = ["toml", "yaml"] }
serde = "1"

[dev-dependencies]
serde = { version = "1", features = ["derive"] }
```

- 공식 `config 0.15.27` manifest에는 Rust 최소 버전 `1.88.0`과 `toml`, `yaml` 기능이 명시되어 있다. 기본 기능은 추가 형식·async 등을 포함한다. 선택한 기능만 활성화하면 이번 필수 형식에 범위를 맞출 수 있다. `0.15.27` 지정은 Cargo 호환 범위이며 정확한 해석 버전은 구현 시 생성되는 `Cargo.lock`으로 기록한다. [공식 manifest](https://raw.githubusercontent.com/rust-cli/config-rs/v0.15.27/Cargo.toml)-`측정함` / 최소 기능 선택-`추론함`
- 로컬 rustc 버전은 위 최소 버전보다 높지만, 모든 전이 의존성의 실제 빌드 가능성을 검증한 것은 아니다. 별도의 프로젝트 MSRV는 이 작업에서 새로 선언하지 않는다.-`측정함`
- 운영 코드에는 `DeserializeOwned` 경계만 필요하고, derive는 테스트 구조체에서 사용한다. 이를 일반 의존성과 개발 의존성으로 나누면 일반 사용에 테스트용 derive 요구를 추가하지 않는다. [Serde derive](https://serde.rs/derive.html)-`추론함`
- `from_env(prefix)`는 파일 없는 사용을 직접 표현하고, `new()` 또는 `Default`와 `env_prefix()` 조합에서 생기는 소스 없는 구성을 공개하지 않는다. 다른 생성 패턴을 추가하지 않는 이유다. 단점은 두 생성자를 제공한다는 점이며, 파일+환경 구성은 기존 메서드를 재사용한다.-`추론함`
- 직접 TOML·YAML 파서를 조합하는 대안은 사용자 지정 `config::Config` 반환·소스 결합 계약을 다시 구현해야 하므로 선택하지 않는다.-`추론함`

## 테스트 설계와 요구사항 추적

테스트는 외부 호출 관점을 확인하도록 `tlab/tests/config.rs`에 둔다. 아래는 검증 계획이며 아직 실행하지 않았다.-`측정함`

| 요구사항 | 검증 대상과 관찰 기준 |
| --- | --- |
| R1, R6 | 공개 `Loader::from_file`로 임시 TOML을 읽고 `name`을 조회한다. |
| R2, R8 | 지정 경로와 의존성이 탐색하는 후보 파일이 모두 없으면 `Error::Internal`이고 파일을 생성하지 않는다. 환경 소스와 조합해도 동일하다. 잘못된 TOML도 `Internal`이다. |
| R3, R4 | 공개 `from_env`로 중첩 키·정수·불리언을 읽는다. 다른 접두사의 변수는 포함되지 않는다. |
| R5 | 파일과 동일한 키를 환경에 지정하면 환경값을 얻고, 파일에만 있는 키는 남는다. 잘못된 파일은 환경 소스가 있어도 오류다. |
| R7 | 테스트 전용 중첩 구조체로 YAML의 문자열·정수·불리언·`PathBuf`를 역직렬화한다. 실제 tracing 모듈은 참조하지 않는다. |
| R8 | 정수 필드에 숫자로 변환할 수 없는 문자열을 주어 `Internal`, 역직렬화 문맥 및 원인 체인을 확인한다. |
| R9 | 기존 오류 테스트·`add` 테스트와 함께 실행한다. 공개 경로와 반환 타입 사용을 컴파일로 확인한다. |

환경변수 테스트는 다음 방식으로 격리한다.

1. 부모 테스트가 `std::env::current_exe()`로 현재 통합 테스트 실행 파일을 얻고, 자식 전용 테스트를 `--exact <이름> --ignored --test-threads=1`로 실행한다.
2. 자식 전용 테스트는 `#[ignore]`로 기본 실행에서 제외하고, 부모가 넣는 전용 실행 표시를 확인하여 잘못된 직접 실행을 실패로 드러낸다. 부모는 `Command::env`로 테스트용 접두사 변수와 시나리오를 지정한다. 해당 접두사의 상속 변수는 `env_remove`로 제외해 기대값을 고정한다.
3. 자식은 공개 Loader API를 호출해 위 환경변수·병합 검증을 수행한다. 부모는 자식의 성공 상태를 단언하고 실패 시 stdout·stderr를 제공한다. 부모 환경을 바꾸지 않으므로 복구용 `EnvironmentVariable`, 프로세스 전역 잠금, unsafe 환경 변경을 추가하지 않는다.

Rust 2024에서는 `set_var`·`remove_var`가 unsafe이다. 테스트에서 공유하는 Mutex만으로 다른 모든 스레드의 환경 접근을 통제한다고 볼 수 없으므로 제시 테스트 도우미를 그대로 만드는 안은 택하지 않는다. [Rust Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/newly-unsafe-functions.html)-`측정함` / 자식 프로세스 선택 이유-`추론함`

`Command::env`는 시작할 자식의 환경을 구성한다. 따라서 실제 환경 소스를 읽는 공개 경로를 검증하면서 부모 테스트 환경을 보존할 수 있다. 의존성의 `Environment::source`로 맵만 주입하는 대안은 실제 공개 Loader 경로를 검증하려면 별도 주입 경계를 추가해야 하므로 선택하지 않는다. [Command::env](https://doc.rust-lang.org/std/process/struct.Command.html#method.env), [Environment::source](https://docs.rs/config/0.15.27/config/struct.Environment.html#method.source)-`추론함`

임시 파일은 표준 라이브러리로 테스트 전용 경로를 배타 생성하고, 테스트 범위 종료 시 정리하는 작은 도우미를 테스트 파일 안에 둔다. 테스트마다 다른 경로를 사용하며 기존 파일을 덮어쓰지 않는다. 정리 실패를 성공으로 보고하지 않는다. 새 테스트 의존성이나 범용 `test_support` 모듈은 만들지 않는다.-`추론함`

후속 구현 검증 명령: `cargo fmt --check`, `cargo test -p tlab`, `cargo clippy -p tlab --all-targets -- -D warnings`. 이것은 실행 계획이며 통과 결과가 아니다.

## 변경 범위와 통합 지점

| 파일 | 책임·변경 | 선행 계약·영향 |
| --- | --- | --- |
| `tlab/src/config.rs` 신규 | Loader와 두 소스 결합·역직렬화·오류 매핑 | 위 공개 API와 R1~R8 |
| `tlab/src/lib.rs` | `pub mod config;` 추가 | 기존 오류 공개와 `add`는 유지 |
| `tlab/Cargo.toml` | config·serde 및 테스트 derive 기능 | Config 반환이 외부 크레이트 타입을 공개하므로 이후 config 버전 변경 시 호출자 호환성 검토 필요 |
| `Cargo.lock` | Cargo가 의존성 해석 결과 갱신 | manifest 변경과 함께 통합·확인 |
| `tlab/tests/config.rs` 신규 | 공개 API·임시 파일·자식 프로세스 검증 | 위 API·오류 계약과 Cargo 테스트 기능 |
| `tlab/src/error.rs`, `tlab/tests/error.rs` | 수정 없이 재사용 | 기존 `Internal` 변형과 오류 원인 체인 계약 유지 |

Task 분할에서는 Cargo·공개 모듈 등록을 통합 지점으로 다룬다. 로더와 테스트는 같은 공개 API·오류 계약에 의존한다. 이 단계에서는 작업 배정·구현 순서를 별도로 만들지 않는다.-`추론함`

## 완료 확인과 후속 검증

- 요구사항 R1~R9를 인터페이스·오류·검증 계획에 연결했고 기존 소스·의존성·도구 버전과 공식 의존성 자료를 확인했다. 운영 코드 변경, 의존성 설치·해석, Cargo 검사와 별도 독립 리뷰는 수행하지 않았다.-`측정함`
- 사용자가 설계안의 완료를 명시하여 `from_env`, 최소 의존성 구성, 자식 프로세스 테스트 방식을 포함한 설계안을 확정했다. 핵심 범위·우선순위·모듈 위치는 완료된 요구사항을 따른다.-`측정함`
- 인터페이스의 입력·출력·오류·부수 효과, 기존 기능 재사용, 변경 범위·통합 지점, R1~R9의 검증 계획이 문서에 정의되어 있다. 구현 방향을 바꾸는 사용자 질문은 남아 있지 않다.-`측정함`
- 설계 상태는 `complete`로 기록한다. 실제 Task 분할·구현·의존성 해석·실행 검증은 후속 단계에서 수행한다.

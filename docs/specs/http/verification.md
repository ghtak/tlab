---
id: VER-HTTP
status: draft
---

# HTTP 구현 검증·리뷰

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| verifies | [SPEC-HTTP](./spec.md) | 단일 서버와 상위 HTTP·HTTPS 조립 요구사항 |
| verifies | [PLN-HTTP](./plan.md) | 최종 공개 API·소유권·종료·TLS 계약 |

## 최신 코드 리뷰 — 명시적 종료 제어

- 대상: `f6c857586adceb62ec6282f08b70feda26076675` → HTTP 라이브러리·앱·관련 테스트·명세의 미커밋 변경. 이번 작업과 무관한 미추적 todo 문서는 제외했다.
- 식별: `Cargo.lock`, `tlab/Cargo.toml`, `tlab/src/http.rs`, `tlab/tests/http.rs`, `tlab-app/src/main.rs` 순서의 경로·NUL·파일 바이트·NUL SHA-256은 `a50532a7f74bdf442dd10b250092fbd3f02920c0529a87350ddc4e2989705fe6`이다.
- 선행 검사: fmt → Clippy → audit 순서로 모두 통과하여 LLM 리뷰를 수행했다. 이전 포맷 실패는 현재 대상에서 해소되었다. 코드 수정은 하지 않았다.
- 판정: **수정 필요**. 초기 제안 배점 기준 **75/100**, **R2**. 주요 발견 사항은 점수와 관계없이 수정 대상이다.

### 발견 사항

1. **주요: provider 기능 조합에 따라 TLS 준비가 panic한다.** [http.rs:124](../../../tlab/src/http.rs)의 `RustlsConfig::from_pem`은 내부에서 Rustls 기본 provider 선택을 사용한다. tlab이 활성화한 ring과 소비자 의존성이 활성화한 aws_lc_rs가 함께 존재하고 앱이 기본 provider를 등록하지 않으면 Rustls 선택 과정과 axum-server의 JoinHandle unwrap에서 panic한다. `Server::new()`가 `Err`를 반환하지 않고 호출 task가 실패한다. 기존 구현은 ring provider를 명시했으므로 이 전역 초기화 전제에 의존하지 않았다. (확인)
   - 재현: `/private/tmp/tlab-review-provider-200zr9zi`의 별도 소비자 crate에 tlab 경로 의존성과 `rustls`의 aws_lc_rs feature를 선언하고 `cargo run --manifest-path /private/tmp/tlab-review-provider-200zr9zi/Cargo.toml --target-dir /Users/codex/works/tlab/target --offline` 실행. Rustls 0.23.46·axum-server 0.8.0에서 종료 코드 101, provider 선택 panic을 확인했다. 소켓은 열지 않았다.
   - 대조: 같은 소비자 main에서 `rustls::crypto::ring::default_provider().install_default()`를 먼저 호출하면 `Server::new()`가 정상 반환했다. 재현 원본은 해당 디렉터리의 `without-provider.rs`, 대조 코드는 `src/main.rs`다.
   - 수정 방향: Rustls 제공 API는 유지하되 provider 초기화 책임·선택 정책을 정하고 앱과 소비자 예제에 반영한다. 라이브러리에서 미충족 전제를 검사해 panic 대신 Result 오류를 전달하거나 명시적 provider 전달을 지원하는 방안을 검토한다. 프로세스 전역 설정을 라이브러리가 무조건 덮어쓰면 안 된다. 앱 초기화 책임은 [Rustls 공식 문서](https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html#using-the-per-process-default-cryptoprovider)와 일치한다.
2. **경미: 테스트 포트 인계에 경쟁 구간이 있다.** [http.rs 통합 테스트:149](../../../tlab/tests/http.rs)의 `drop(reservation)` 뒤 별도 task가 같은 주소에 다시 bind한다. 그 사이 다른 프로세스·병렬 테스트가 포트를 확보하면 정상 코드에서도 기동 실패나 준비 대기의 오판이 가능하다. 이번 실행에서는 재현되지 않았으며 코드 흐름에서 도출한 테스트 안정성 문제다. 바인딩된 포트 0의 실제 주소를 동기화하여 받는 방식이나 충돌 시 예약·기동 재시도를 고려한다. (추론)

### 품질 점수

| 항목 | 점수 / 4 | 근거 |
| --- | ---: | --- |
| 기능 | 2 | 기본 HTTP·HTTPS·종료·재로드 계약은 통과하지만 소비자 TLS 준비의 반환 계약에 보완 필요 |
| 가독성과 구조 | 4 | 앱의 종료 요청과 라이브러리 실행·TLS 책임이 분리되고 중복 설정 조립 제거 |
| 안정성 | 2 | provider 조합에 따른 준비 단계 panic 재현. 종료·취소·재로드 실패 보존은 기존 테스트로 확인 |
| 보안 | 4 | audit 통과, 검증된 키파일 스냅샷·비밀값 비노출·실패 시 활성 TLS 보존 유지. 발견한 provider 문제는 가용성 문제로 평가 |
| 성능 | 4 | 요청 경로에 새 blocking I/O 없음. 키파일 작업은 spawn_blocking, 재로드만 mutex 적용. 부하 측정은 미수행 |
| 테스트 용이성 | 3 | 실제 HTTP·HTTPS·취소·기한·TLS 교체 검증 유지. 포트 인계 경쟁과 provider 조합 회귀 검증 보완 필요 |

### 검사·리스크·미확인 범위

- `cargo fmt --all -- --check`: 종료 코드 0. `cargo clippy --workspace --all-targets --locked -- -D warnings`: 종료 코드 0·경고 없음. `cargo audit`: 종료 코드 0, 266개 의존성, 1,403개 advisory. DB 기준 `965bf9ad083b58e6340d65829d85217b18be9290`.
- `cargo test --workspace --locked`: 59개 통과, 기존 ignored 5개. 실제 소켓 테스트는 승인된 환경에서 수행했다. Rust/Cargo 1.97.1, Darwin arm64, 기본 워크스페이스 feature 기준이다.
- Miri 비대상: unsafe·수동 메모리 변경 없이 Tokio·백엔드의 동기화와 소켓 동작을 조립하는 변경이며 실제 연결·취소 테스트로 확인했다. Geiger는 비필수 참고 도구로 미실행했다.
- 복구 가능성: 앱과 라이브러리 호출부를 함께 롤백하면 복구 가능으로 판단한다. 설정 스키마·DB·외부 발송의 변경이 없고 개발용 TLS 파일은 이전 키파일 계약으로도 읽을 수 있다. 실제 롤백은 수행하지 않았다. (추론)
- 아키텍처 변경 있음: `bind/serve`에서 `new/run/graceful_shutdown`으로 공개 API와 준비·바인딩·종료 제어 경계가 바뀌었다. 워크스페이스 앱·테스트는 함께 이행되어 있다. 외부 소비자는 API 수정 및 provider 초기화 전제 확인이 필요하다. 양방향 복구·구조 변경에 따라 R2로 분류한다. (확인·추론)
- 미확인: 운영 배포·외부 소비자 전수 호환성·Windows 실행·부하·롤백 실연습·PostgreSQL 실연동. 기본 feature 테스트 통과가 모든 소비자 feature 조합의 통과를 의미하지 않는다.

## 이전 코드 리뷰 — 선행 검사 실패

- 대상: `f6c857586adceb62ec6282f08b70feda26076675` → 현재 미커밋 HTTP·앱 변경. `Cargo.lock`, `tlab/Cargo.toml`, `tlab/src/http.rs`, `tlab/tests/http.rs`, `tlab-app/src/main.rs` 순서로 각 경로·NUL·파일 바이트·NUL을 연결한 SHA-256은 `33519f89bc9095308b87a5512c7171a0bdd1f89199cff72fb29c0ccaca510176`이다.
- `cargo fmt --all -- --check`: 종료 코드 1. `tlab-app/src/main.rs:68`의 `let http =  prepare(...)`에서 불필요한 공백 1개를 지적했다.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: 통과. 포맷 검사 실패 후 중단 조건을 적용하기 전에 시작된 검사 결과다. `cargo audit`은 미실행이다.
- 판정: 선행 검사 기준의 수정 필요. 코드 리뷰 스킬의 필수 검사 통과 조건에 따라 LLM 리뷰·품질 점수·리스크 평가는 미진행이다. 이전 테스트 통과 기록으로 현재 포맷 실패를 대체하지 않는다.
- 리뷰 요청 범위에 따라 코드를 수정하지 않았다. 공백 정리 후 선행 검사를 순서대로 완료하고 코드 리뷰를 진행해야 한다. (확인)

## 상위의 명시적 종료 제어 확인

- run(router)의 shutdown 인자와 내부 종료 대기를 제거하고 graceful_shutdown()을 공개했다. 앱은 OS 신호 후 해당 메서드를 호출하고 기존 run 완료를 기다린다.
- cargo test --workspace --locked: 59개 통과, 기존 ignored 5개. cargo clippy --workspace --all-targets --locked -- -D warnings 통과. 실제 앱의 HTTP·HTTPS 응답과 SIGTERM 종료는 [최신 앱 실행 기록](./app/verification.md#명시적-graceful_shutdown-적용-확인)에 남긴다. (확인)

아래는 이전 인터페이스의 확인 기록이다.

## 서버 인터페이스 정리 구현 확인

이번 결과는 `new/run/reload_tls_config`와 RustlsConfig의 PEM API를 사용하는 미커밋 변경 기준이다. 아래 이전 검증·리뷰는 과거 버전에 관한 기록이며 이번 변경의 리뷰 결과가 아니다. (확인)

- `cargo test --workspace --locked`: 59개 통과, 기존 ignored 5개, 실패 없음. 실제 HTTP·HTTPS, 정적 파일, TLS 재로드 성공·실패·취소, 기존 연결 유지, graceful 종료·기한 후 연결 종료·실행 취소를 확인했다.
- `cargo fmt --all -- --check`: 통과. `cargo clippy --workspace --all-targets --locked -- -D warnings`: 통과, 경고 없음.
- 생성 시 미바인딩, 실행 시 포트 충돌, 충돌 해소 후 같은 서버 재실행을 확인했다. 종료 기한 테스트는 백엔드의 강제 종료 후 `Ok(())` 반환을 기대한다.
- 로컬 포트 바인딩은 샌드박스에서 차단되어 승인된 실행 환경에서 테스트했다.
- 키파일 생성·검증은 기존 모듈에 남기고 PEM 파싱·TLS 설정·ALPN은 axum-server에 맡겼다. `TlsState`·`load_tls`·별도 종료 helper를 제거했다.
- 사용자 요청 범위는 설계·구현까지이며 별도 자체 점검·정식 리뷰·audit은 수행하지 않았다. 후속 앱 적용 요청의 실제 실행 결과는 [앱 확인 기록](./app/verification.md#newrun-적용-후-실제-앱-확인)을 참조한다. (확인)

## 이전 버전 검증·리뷰

## 대상과 환경

- 비교 기준: `2991b82f0f95c6f7b9534d1137894a9c7168d95e` → 미커밋 변경. 새 HTTP 소스·테스트와 Cargo 의존성, 관련 문서를 포함한다.
- 최종 구조: 리스너 하나를 소유하는 `Server`, 선택적 TLS, 별도 정적 Router·TLS 재로드. 다중 인스턴스 정책은 상위 책임이다.
- 코드 식별: `Cargo.lock`, `tlab/Cargo.toml`, `tlab/src/lib.rs`, `tlab/src/http.rs`, `tlab/tests/http.rs` 순서로 각 경로·NUL·파일 바이트·NUL을 연결한 SHA-256은 `79f6c6f9d3d422a3acd2c7bea7be7182cddfde3a4ec13851c7b42a7fc44d4c28`이다.
- 실행일: 2026-11-04, Darwin arm64. Rust 1.97.1, Cargo 1.97.1, cargo-audit 0.22.2. 기본 feature, 로컬 loopback·임시 파일을 사용했다.
- 최초 loopback 테스트는 샌드박스의 포트 바인딩 차단으로 실패했다. 권한 승인 후 실제 연결 테스트를 다시 실행했으며 아래 결과는 승인 후 최종 코드 기준이다. (확인)

## 실행 결과

| 검사 | 명령 | 최종 결과 |
| --- | --- | --- |
| 포맷 | `cargo fmt --all -- --check` | 통과 |
| Clippy | `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 통과, 경고 없음 |
| 의존성 보안 | `cargo audit` | 종료 코드 0, 차단 advisory 없음. 266개 의존성 검사 |
| 회귀·통합·문서 테스트 | `cargo test --workspace --locked --offline` | 59개 통과, 5개 ignored, 실패 없음 |

audit은 RustSec DB `965bf9ad083b58e6340d65829d85217b18be9290`의 1,403개 advisory로 수행했다. 이후 구조 변경에서 Cargo 의존성과 잠금 파일은 바뀌지 않아 같은 결과를 재사용했다.
ignored 5개는 기존 PostgreSQL 실연동 4개와 부모 테스트가 격리 자식 프로세스로 실행하는 환경변수 보조 테스트 1개다. PostgreSQL 4개를 실행했다고 보고하지 않는다. (확인)

새 코드에 unsafe·수동 메모리 조작은 없다. 동시성은 Tokio mutex·future 취소와 Rust 소유권을 사용하며 실제 소켓 테스트를 수행했으므로 이번 변경에서 Miri는 비대상으로 판단했다.
Geiger는 필수 검사가 아니므로 실행하지 않았다. 의존성에 unsafe가 없다는 의미는 아니다. (확인·추론)

## 요구사항별 근거

HTTP 통합 테스트는 [`tlab/tests/http.rs`](../../../tlab/tests/http.rs), 내부 실행 실패·재로드 대기 취소 테스트는 [`http.rs`의 테스트](../../../tlab/src/http.rs)에 있다. (확인)

| 요구사항 | 확인한 결과 |
| --- | --- |
| REQ-HTTP-001·002 | 앱이 선택한 HTTP 단독·HTTPS 단독·동시 인스턴스에 같은 Router를 전달. 상태 의존 응답·공통 헤더·정적 파일이 동일. 앱의 빈 인스턴스 설정 거부 예시 검증 |
| REQ-HTTP-003 | 신뢰 저장소에 테스트 인증서를 등록한 실제 TLS 연결 성공. 누락 파일·잘못된 TLS 경로 준비 실패. 기존 키파일 테스트 16개 회귀 통과 |
| REQ-HTTP-004 | GET·HEAD·index·404·405, 기존 API, 접두사·디렉터리 오류, 일반·인코딩 경로 이탈, Unix 심볼릭 링크의 외부 파일 응답 확인 |
| REQ-HTTP-005 | 준비 중에는 응답 없음, drop 시 포트 해제. 앱의 두 번째 준비 실패 시 첫 포트 해제. 진행 중 응답의 graceful 종료, 기한 초과 오류·HTTP 연결 정리, 미완료 TLS가 있어도 각 서버 반환 기한 준수. 앱의 `try_join!` 실패 시 다른 실행 취소 |
| REQ-HTTP-006 | 공개 `crate::Result`, 입력·상태 오류 분류, 백엔드 실패 원인·예기치 않은 종료 전달, 잘못된 PEM 내용의 비밀값 비노출 |
| REQ-HTTP-007 | A→B 재로드 후 새 핸드셰이크의 실제 인증서가 B. 기존 TLS 연결·별도 HTTP 인스턴스 정상. 동시 재로드 성공. 키 불일치·손상·파일 누락 시 B 유지, 생성 옵션이 있어도 재로드 중 파일 미생성. 대기 중 재로드 취소 후 활성 설정 유지·후속 재로드 성공 |

TLS 클라이언트 검증을 끄지 않았고 인증서 교체 확인에는 새 클라이언트 설정·새 연결을 사용했다.
이미 수락된 미완료 TLS 핸드셰이크의 즉시 취소는 최종 계약에서 제외했다. 해당 테스트는 서버 작업의 반환·리스너 해제를 확인하고 클라이언트 연결을 정리한다. (확인)

## 코드 리뷰

- 선행 검사: 위 필수 검사 통과 후 LLM 자체 리뷰 수행. 최종 구조 변경 후 포맷·Clippy·전체 테스트를 재실행했다.
- 발견 사항: 최종 코드에서 치명·주요·경미 결함을 발견하지 못했다. 이전 다중 서버 초안에서 확인했던 종료 중 실패의 작업 회수 문제는 최종 단일 서버 구조에서 해당 감독 코드 자체가 제거되었다.
- 판정: 통과. 가이드의 초기 제안 배점 기준 **92.5 / 100**.
- 리스크: **R2**. 새 공개 모듈·전송 계층과 상위의 다중 서버 책임 경계가 추가된다. 기존 앱 호출자·설정·DB에는 변경이 없다.
- 복구 가능성: 이번 저장소 변경은 코드·의존성 복원으로 기존 앱 동작으로 돌아갈 수 있는 양방향 변경으로 판단한다. 실제 롤백 실행은 하지 않았다. (확인·추론)

| 항목 | 점수 / 4 | 근거·검증 범위 |
| --- | ---: | --- |
| 기능 | 4 | 단일 HTTP·HTTPS 실행, 상위 다중 조립, 정적 파일, 실제 TLS 교체와 실패 보존 확인 |
| 가독성과 구조 | 4 | `Config`와 `Server`는 리스너 하나만 표현. `local_addr()` 통합. 다중 서버 supervisor·JoinSet·연결별 취소 wrapper 제거 |
| 안정성 | 3 | 실제 종료·부분 준비 실패·재로드 오류·대기 취소 검증. blocking 작업 실행 도중 취소는 직접 강제하지 않고 활성 설정 교체 위치를 코드로 확인 |
| 보안 | 4 | 신뢰 검증 TLS, ring 명시, PEM 오류 정제, 재로드 중 파일 미생성, 정적 파일 접근 경계 검증·문서화, audit 통과 |
| 성능 | 3 | 동기 TLS 로드를 blocking pool로 분리하고 연결별 추가 감시·박싱 제거. 부하·지연 벤치마크는 미실시 |
| 테스트 용이성 | 4 | 임시 포트·인증서·디렉터리, 상위 종료 future, clone 가능한 재로드 객체로 계약·실패 재현 가능 |

### 복구·전환과 미확인 범위

- 전환: 실제 앱 연결은 별도 작업이다. 연결하는 앱이 인스턴스 설정 검증·준비 순서·그룹 실패·종료 정책을 선택해야 한다. 이 저장소의 앱 기동은 변경하지 않았다.
- 복구: 이번 코드·Cargo 변경을 되돌리면 기존 앱 경로로 복원된다. 데이터 마이그레이션·외부 발송은 없다. 개발용 생성 파일은 opt-in이며 기존 파일을 덮어쓰지 않고, 이후 앱에서 생성했다면 파일 보존·삭제는 운영자가 선택한다.
- 미확인: PostgreSQL 실연동 4개, Windows 동작, HTTP/2 클라이언트 연동, 처리량·지연 부하 측정, blocking TLS 로드 도중의 취소 강제 재현은 미수행이다. 기본 HTTP/1.1 실제 연결과 최종 변경 범위에 대해 판정했다.
- 인프라: 미완료 TLS 연결의 즉시 차단·외부 드레이닝은 이번 라이브러리에서 보장하거나 실제 프록시로 검증하지 않았다. (확인·추론)

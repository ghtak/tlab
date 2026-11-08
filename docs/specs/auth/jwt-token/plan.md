---
id: PLN-AUTH-JWT-TOKEN
status: accepted
---

# JWT 토큰 기반 구현·검증

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| realizes | [SPEC-AUTH-JWT](../jwt.md) | Access 클레임·서명 및 Refresh 난수·해시 |
| follows | [PLN-AUTH-JWT-TODO](../jwt-todo.md) | 구현 순서 중 토큰 기반 부분 |

## 목적·승인·완료 조건

사용자는 이번 요청에서 토큰 기반 기능의 조사·설계·구현·검증과 자체 점검을 승인하고 단계별 추가 승인을 생략하도록 지정했다. 이 범위의 판단 기준으로 채택한다. 커밋과 추가 subagent 실행은 하지 않는다. (확인)

기존 `Ed25519KeyFile`/`Ed25519Material`을 재사용하여 Access를 발급하고 공개키만으로 검증한다. 실제 서명·검증, 잘못된 서명·키·알고리즘·발급자·대상·필수 클레임의 거부, 정확한 10분 경계, Refresh 난수·해시를 자동 검증하는 것이 완료 조건이다. (확인)

## 책임과 배치

- `tlab-app/src/auth/token.rs`: auth 공용 토큰 계약·구현. `AccessIssuer`, `AccessVerifier`, `AccessClaims`, 비밀 토큰 wrapper와 `RefreshHash`를 제공한다. 시나리오·HTTP·저장소에 의존하지 않는다. (확인)
- `tlab/src/keyfiles/ed25519.rs`: 기존 키 읽기·생성·키 쌍 검증 책임을 그대로 사용한다. 키 로더는 수정하지 않는다. (확인)
- 로그인·갱신 호출자: 신뢰할 수 있는 계정·사용자 정보, 로그인별 UUID v4, 실제 현재 시각을 제공한다. 토큰 기반 최초 구현 단계에서는 Refresh 저장·7일 만료·폐기와 상위 설정 조립을 후속 작업으로 구분했다. 현재 연결은 아래 후속 문서를 따른다. (확인)

토큰 기반 최초 구현 단계에서는 기존 auth에 토큰 공용 작업이 없어 token 모듈과 공개 선언을 추가했다. 당시에는 호출자가 없어 Container·설정·라우터를 변경하지 않고 후속 연결 전 dead-code 허용 사유를 모듈에 한정했다. 현재는 앱과 로그인·갱신 경로에 연결되어 있으며 당시 미연결 설명은 현재 앱의 상태를 뜻하지 않는다. 계층이나 trait을 추가하지 않아도 실제 키와 명시적 시간으로 토큰 계약을 검증할 수 있다. (확인·추론)

## Access 계약과 결정

| 항목 | 계약 |
| --- | --- |
| 알고리즘 | JWT 헤더 `EdDSA`, Ed25519 PEM 사용. 발급과 검증 모두 고정하며 헤더의 키 선택 정보는 사용하지 않음 |
| 키 | 발급기는 검증된 `Ed25519Material`의 개인키 사용. 검증기는 신뢰된 Ed25519 SPKI 공개키 PEM만 필요 |
| `iss`, `aud` | 생성 시 주입하는 비어 있지 않은 단일 문자열. 검증 시 설정값과 일치해야 함 |
| `sub` | 비어 있거나 공백뿐인 값이 아닌 문자열. 기존 계정 ID는 호출자가 문자열로 구성 |
| `sid` | RFC4122 variant의 UUID v4. 호출자가 생성·유지하며 발급 시 하이픈 포함 문자열로 직렬화 |
| `iat`, `exp` | 음수·소수 없는 Unix UTC 초(`u64`). `iat <= now < exp`, `exp == iat + 600`. 덧셈 overflow 거부 |
| `user` | 필수 JSON 객체(`Map<String, Value>`). null·배열·JSON을 담은 문자열 거부. 구체 필드와 권한 원본은 호출자 정책에서 확정 |
| 결과 | 검증 성공 시에만 `AccessClaims` 반환. 서명·클레임 실패는 원문 없는 `InvalidAccess`로 통일 |

위 계약은 구현과 테스트로 확인했다. `user` 안의 계정 ID 중복이나 권한 필드 스키마는 이 기반 모듈에서 추측해 추가하지 않는다. `sub`가 계정 식별 기준이며, 실제 최신 사용자 정보를 구성하는 책임은 후속 로그인·갱신 시나리오에 있다. (확인·추론)

10분은 발급 시각부터 정확히 600초다. 시계 오차 유예는 0초이며 만료 시각과 같으면 거부한다. 미래 `iat`, 600초보다 짧거나 긴 수명도 거부한다. 발급·검증에 같은 명시적 `now` 전달 방식을 사용하며 전역 환경이나 시스템 시계를 테스트에서 교체하지 않는다. (확인)

서명은 `jsonwebtoken 9.3.1`의 ring 구현에 맡긴다. 이 버전의 [Validation 계약](https://docs.rs/jsonwebtoken/9.3.1/jsonwebtoken/struct.Validation.html)은 내부 시스템 시계를 사용하므로 `validate_exp`만 끄고, 서명·알고리즘·발급자·대상 검증은 유지한다. 이후 타입 역직렬화로 모든 필수 필드와 타입을 확인하고 명시적 시간 정책을 적용한다. 라이브러리의 insecure signature disable API는 사용하지 않는다. 캐시에 있는 기존 ring 계열과 호환되는 버전으로, 의존성 해결 시 새 패키지는 jsonwebtoken·pem·simple_asn1 세 개만 추가되었다. (확인)

## Refresh 계약과 비밀값 처리

OS 난수 `getrandom`으로 32바이트(256비트)를 생성하고 `base64`의 URL-safe no-padding으로 인코딩한다. 결과는 43문자이며 `RefreshToken::generate()`가 반환한다. SHA-256은 `sha2`에 맡기며, DB 조회용 `RefreshHash::of(&str)`는 전달받은 토큰 문자열의 정확한 UTF-8 바이트를 해시한 32바이트를 제공한다. 원문 trim·정규화는 하지 않는다. 해시 생성은 원문의 만료·폐기 상태를 검증하는 연산이 아니다. (확인)

동일 Refresh로 반복해서 같은 해시를 얻는다. 교체·소비 상태·재사용 감지는 구현하지 않는다. 최초 로그인부터 7일 만료를 유지하는 정책은 저장 및 갱신 시나리오에서 적용해야 하며, 이번 난수·해시 모듈에는 저장 상태나 만료 시각이 없다. (확인·추론)

발급기·검증기·Access/Refresh 원문 wrapper·claims·RefreshHash에는 Debug를 구현하지 않는다. 원문 wrapper에는 Serialize도 없으며 `expose_secret()`으로 의도적으로 꺼내야 한다. 토큰·키를 출력하는 로그와 원문이 포함될 수 있는 라이브러리 오류 source를 만들지 않는다. 공개 오류는 고정된 메시지와 구분값만 가진다. 메모리 zeroize는 이번 계약에 포함하지 않는다. (확인)

## 검증 결과

토큰 테스트 7개가 통과했다. 테스트는 임시 디렉터리의 실제 `Ed25519KeyFile.load()`로 독립된 키 쌍을 생성하며, mock 서명 결과를 사용하지 않는다. 테스트 입력 시각은 고정하여 실제로 10분을 기다리지 않는다. (확인)

| 검증 | 확인 범위 |
| --- | --- |
| 정상 발급·검증 | 모든 클레임 왕복, user 객체 유지, 공개키 단독 검증, EdDSA 헤더, 599초 성공·600/601초 거부 |
| 서명·형식 | payload 변조, signature 변조, 다른 실제 키의 서명, 깨진 JWT와 빈 서명 거부 |
| 알고리즘 | 공개키 PEM으로 만든 HS256 토큰 및 alg=none 토큰 거부 |
| 필수 클레임 | 7개 필드 각각 누락·null·배열·boolean 거부 |
| 정책 | 다른 iss/aud, 공백 sub, 잘못된 UUID·nil·v1, 문자열 user, 미래/음수/소수 iat, 만료·잘못된 수명·overflow 거부 |
| 설정·입력 | 빈 iss/aud, 잘못된 PEM·개인키를 공개키로 사용, 빈 sub·잘못된 sid·발급 시간 overflow 거부. 오류에 키 원문 미포함 |
| Refresh | 실제 생성 결과의 길이·문자 집합·32바이트 복원·독립 생성 구분, 동일 원문 반복 해시, 공백 변경의 구분, SHA-256 `abc` 알려진 정답 벡터 |

실행 명령과 전체 회귀 결과는 최종 실행 완료 후 아래 구현 기록에 남긴다.

## 범위 밖·후속 작업

토큰 기반 최초 구현 단계에는 DB entity/repository/migration, Refresh 저장·7일 만료·폐기 판단, 로그인 usecase, HTTP/Bearer/cookie/CSRF, JWKS·키 교체·Container 조립을 포함하지 않았다. 당시에는 실제 로그인 토큰 반환과 DB Refresh 인증이 없었으며 `user`의 업무 필드·권한 모델도 미정이었다. 이는 최초 단계의 범위 기록이다. (확인)

현재 DB 저장은 [Refresh 저장](../jwt-refresh-storage/plan.md), 로그인 발급은 [로그인 발급](../jwt-login/plan.md), 갱신·종료는 [lifecycle](../jwt-refresh-lifecycle/plan.md), HTTP·Bearer·Container 조립은 [JWT API](../jwt-api/plan.md)에서 이어 구현했다. 로그인·갱신의 `user`에는 서버 계정의 nickname·status를 사용한다. 웹 쿠키·CSRF와 공개키 배포·교체, 권한 모델은 후속 범위다. 과거 검증·리뷰 결과는 해당 단계의 대상·시점으로 보존한다. (확인)

## 구현·변경 기록

1. 사용자 요청 범위에 따라 기존 키 타입·auth 구조·구현/테스트 가이드를 조사하고 위 계약으로 구현했다. `AGENTS.md`는 수정하지 않고 미추적 `jwt-todo.md`의 기존 내용을 보존한다. 최초 토큰 테스트 실행은 테스트 파일 작성 완료 전 시작되어 E0583(테스트 모듈 파일 없음)으로 종료했으며, 파일 작성 후 재실행한 7개 테스트는 모두 통과했다. (확인)
2. 최종 실행 결과: (확인)
   - `cargo test -p tlab-app auth::token --offline`: 7 passed, 0 failed, 16 filtered out.
   - `cargo test --workspace --locked --offline`: 전체 87 passed(문서 테스트 8개 포함), 0 failed, 8 ignored. ignored 중 7개는 기존 PostgreSQL 연동 테스트이며 이번 범위에서 실행하지 않았다. 나머지 1개는 config 환경변수 시나리오용 helper로, 기존 부모 테스트가 격리 자식 프로세스에서 실행한다.
   - `cargo fmt --all -- --check`: 통과.
   - `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: 통과.
   - 작업 대상 `git diff --check`: 통과. 전체 diff에는 사용자가 기존 수정한 `AGENTS.md`의 trailing whitespace가 있어 그대로 보존했다.
3. 자체 점검에서 토큰 원문·개인키 Debug/Serialize/로그, 알고리즘 선택 가능성, 시스템 시간 의존, 요구된 실패 경로, 기존 키 로더 재사용과 범위 밖 변경 여부를 대조했다. 구현 범위 내 미완료 사항은 없다. 기존 DB 테스트와 앱 인증 흐름은 검증한 것으로 보고하지 않는다. `jwt-todo.md`에는 아래 구현 결과의 링크·요약만 추가하며 기존 계획은 유지했다. 커밋과 추가 subagent는 생성하지 않았다. (확인)


### 코드 리뷰

#### 발견 사항

- **[경미·진단 보완] 잘못된 길이의 공개키도 검증기 생성에 성공한다.** 위치: `tlab-app/src/auth/token/mod.rs:121`–`122`, 관련 테스트 `tlab-app/src/auth/token/tests.rs:185`–`188`. Ed25519 OID와 ASN.1 형식은 맞지만 공개키 BIT STRING이 32바이트가 아닌 PEM을 설정하면 `AccessVerifier::new()`가 `Ok`를 반환한다. 임시 재현에서는 공개키가 1바이트인 DER `300b300506032b657003020001`을 `PUBLIC KEY` PEM으로 감쌌으며, 생성 성공 후 정상 토큰 검증은 `InvalidAccess`로 실패했다. `jsonwebtoken 9.3.1`의 `DecodingKey::from_ed_pem`은 OID 분류와 BIT STRING 추출까지만 수행하고 Ed25519 키 길이는 확인하지 않는다. 따라서 설정 손상 시 원인을 설정 오류로 구분하지 못하고 모든 토큰을 잘못된 토큰으로 처리하게 된다. 인증 우회는 없으며, 신뢰된 유효 SPKI를 전달한다는 현재 API 전제를 고려하여 차단 결함이 아닌 진단 개선으로 분류한다. 수정 방향: 생성 경계에서 Ed25519 SPKI 구조·키 길이를 검증하고 `InvalidConfiguration`으로 반환하며, 형식은 맞지만 길이가 틀린 키의 생성 실패 테스트를 보완한다. 기존 테스트는 일반 텍스트와 개인키만 거부하므로 이 조건을 검출하지 못한다. (확인: 재현·라이브러리 소스 / 추론: 설정 장애의 진단 영향)

치명·주요 결함은 발견하지 않았다. 위 항목 외에 미합의된 `user` 스키마, 토큰 크기 제한, zeroize, `nbf`·`typ` 추가 검증을 필수 정책으로 가정하지 않았다. DB·로그인·HTTP·JWKS 미연계와 Refresh 교체·재사용 감지 미구현은 요청한 범위 밖이므로 결함으로 평가하지 않았다. (확인·추론)

#### 대상과 판정

- 비교 기준: `HEAD e2ff6a19c6306c2a04d3869593d58249f144e9c4` → 2026-11-08의 미커밋 작업 트리. 대상은 `auth/token/mod.rs`, `auth/token/tests.rs`, `auth/mod.rs`, `tlab-app/Cargo.toml`, `Cargo.lock` 및 이 계획 문서다. 기준 문서는 `jwt.md`, `jwt-todo.md`와 이번 요청의 최신 범위다. (확인)
- 검토 전 SHA-256: `token/mod.rs` = `6cc3526c0661ebb12df51da9837bd6e34ebf5658ffc3d95c088415f2dbe048c2`, `token/tests.rs` = `8b2f3718f228f2bb7d780776ed82b837ffab5644aa7915c533af4d6307591e7a`, `Cargo.lock` = `1d5da3606d4ce03e914adf578f78dfc77de800ef0965ceeaac569e1d194207e4`, `tlab-app/Cargo.toml` = `679e28d18ab9efcb6deb259cded11b52a9e9493bc91fa74d1486edd0efabf557`, `auth/mod.rs` = `68e913da1cebd43ce47eb602b83a26793909a3622d44f5e59349dd9144c3e915`. 본 절 추가 전 계획 문서 = `7654b9d916fdcc8c6b84bfe16647af3a76b500a59b32cf8b9cd231ab499290a5`. (확인)
- **판정: 통과, 86.25/100.** 가이드 초안의 기준을 적용했다. 모든 항목 3점 이상, 80점 이상이며 치명·주요 발견 사항은 없다. 토큰 기반 범위에 한정한 리뷰 판정이다. (추론)
- 복구 가능성: **양방향·코드 롤백만으로 가능**. 이번 변경은 기존 auth 경계 안에 호출되지 않는 토큰 기능과 의존성을 추가한다. DB 쓰기·설정 전환·외부 발송·실제 발급 경로가 없으므로 코드와 Cargo 파일을 이전 버전으로 되돌리면 복구할 수 있다. 실제 롤백은 실행하지 않았으며, 후속 로그인·저장·외부 토큰 발급 연결 뒤의 복구까지 포함하지 않는다. (확인: 호출 경로·diff / 추론: 복구 가능성)
- 아키텍처 변경: **없음**, 리스크 **R1**. 새 내부 토큰 API는 추가되지만 기존 auth와 키파일의 책임·소유권·의존 방향, 기존 시나리오 계약·주요 처리 흐름·배포 단위는 바뀌지 않는다. 기존 `tlab` 키 검증 책임을 재사용하고, 앱의 토큰 정책은 auth 안에 유지한다. (추론)

#### 필수 선행 검사

검사는 워크스페이스 루트에서 기본 feature·호스트 `aarch64-apple-darwin`으로 아래 순서대로 새로 실행했다. 기존 문서에는 audit 근거가 없어 누락 검사를 포함해 직접 확인했다. 도구 체인은 `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)`, `cargo-audit 0.22.2`다. (확인)

| 검사 | 결과와 근거 |
| --- | --- |
| `cargo fmt --all -- --check` | 종료 0, 출력 없음 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 종료 0, 경고 없음. `/private/tmp/tlab-jwt-review.zNGki0/clippy.log` |
| `cargo audit` | 종료 0, 차단 advisory 없음. 1422 advisory 로드, 294 의존성 검사. `/private/tmp/tlab-jwt-review.zNGki0/audit.log` |

Advisory DB는 실행 중 갱신했으며 기준 커밋은 `309392b18f6b17022f82c701f300433126901068`(커밋 시각 `2026-11-06T16:34:32+01:00`)이다. 실행 완료 확인 시각은 `2026-11-08T03:14:49Z`다. cargo-deny 대체 정책은 확인되지 않았다. Miri는 비대상이다: 변경한 앱 코드에 unsafe·메모리 조작·동시성 안전성 전제 변경이 없고, 암호 연산은 기존 ring 계열의 안전한 API로 호출한다. Geiger는 참고 검사이므로 이번에는 실행하지 않았으며 의존성 내부 unsafe 부재를 주장하지 않는다. **모든 필수 검사 통과 후 LLM 리뷰를 진행했다.** (확인·추론)

#### 항목별 점수

| 항목 | 점수 / 4 | 가중 환산 | 근거·남은 사항 |
| --- | ---: | ---: | --- |
| 기능 | 3 | 18.75 / 25 | Ed25519 Access 발급·공개키 검증, 600초와 7개 필수 필드, Refresh 256비트 난수·SHA-256 충족. 잘못된 공개키의 생성 오류 구분은 보완 가능 |
| 가독성과 구조 | 4 | 15 / 15 | 발급·검증·비밀 wrapper·해시의 책임이 작고 명확함. 키 로딩 재사용, 시나리오·HTTP·DB 의존 없음 |
| 안정성 | 3 | 15 / 20 | 시간 덧셈 overflow·입력 오류 처리와 명시적 시각 사용. 잘못된 길이의 키를 생성 시점에 발견하지 못하는 진단 제한 |
| 보안 | 4 | 20 / 20 | 알고리즘·신뢰키 고정, 실제 서명 후 클레임 검증, 실패 시 원문·PEM·source 미노출. 무효 키도 인증을 거부함 |
| 성능 | 4 | 10 / 10 | 키의 PEM 파싱·검증 설정은 생성 시 수행. 요청별 파일·DB I/O·상태 잠금 없음. 서명 검증·JSON 처리·해시는 입력 크기에 비례하며 구체적인 회귀 근거 없음 |
| 테스트 용이성 | 3 | 7.5 / 10 | 실제 키와 명시적 시간으로 성공·거부를 재현 가능. 기존 테스트의 잘못된 키 입력이 단순 형식 오류에 한정되어 이번 실패 조건을 놓침 |

총점은 `18.75 + 15 + 15 + 20 + 10 + 7.5 = 86.25`다. 기능·안정성·테스트 항목의 보완점은 같은 키 진단 문제의 서로 다른 영향이며 별도 발견 사항으로 중복 집계하지 않았다. 성능 점수는 코드 경로 평가이며 처리량 벤치마크 결과가 아니다. (추론)

#### 실제 검증 동작과 추가 재현

- 로컬 설치 소스 `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/jsonwebtoken-9.3.1/src/{decoding.rs,validation.rs,crypto/eddsa.rs,pem/decoder.rs}`를 확인했다. `decode`는 헤더 알고리즘 허용 목록·키 계열·ring Ed25519 서명을 확인한 뒤 payload를 `AccessClaims`와 라이브러리 검증 타입으로 역직렬화한다. `iss`·`aud`는 필수 및 설정값 검증을 유지한다. 라이브러리가 배열 aud나 일부 숫자 변환을 허용해도 앱의 `String`·`u64` 역직렬화가 해당 타입을 거부한다. `validate_exp=false`, 기본 `validate_nbf=false`이므로 라이브러리 시스템 시계는 호출하지 않으며, 앱이 `iat <= now < exp`와 정확한 600초를 확인한다. (확인)
- `cargo test -p tlab-app auth::token --locked --offline`: **7 passed, 0 failed**. 실제 키 쌍을 사용한 변조·다른 키·HMAC/none·필수 필드·정책·만료 경계·Refresh 해시를 직접 재실행했다. 로그: `/private/tmp/tlab-jwt-review.zNGki0/token-tests.log`. (확인)
- 임시 crate `/private/tmp/tlab-jwt-review.zNGki0`에서 원본 모듈을 절대 경로로 포함해 소스 복사·수정 없이 재현했다. 원본 잠금 파일을 임시 crate에 복사해 시작했고, 동일 `jsonwebtoken 9.3.1`, `uuid 1.26.0`, `getrandom 0.2.17`, `sha2 0.10.9`를 사용했다. 실행: `cargo test --offline --target-dir /Users/codex/works/tlab/target -- --nocapture`. **원본 7개 + 추가 2개 = 9 passed**. 이 중 키 테스트는 잘못된 생성 성공을 확인하도록 작성했으므로, 9개 통과가 해당 개선점의 해소를 뜻하지 않는다. 로그: `/private/tmp/tlab-jwt-review.zNGki0/probe.log`. (확인)
- 추가 검증: Unix epoch 0과 `u64::MAX - 600`에서 정상 발급·599초 허용·600초 거부, 이스케이프된 issuer·한글 audience, 599초 수명·소수/음수 exp·문자열 iat·잘못된 UUID variant·단일 원소 aud 배열·배열 user 거부, 중복 iss와 배열 형태 최상위 payload 거부. 토큰 헤더의 `kid`·`jku`가 검증키 선택에 관여하지 않는 것도 확인했다. (확인)
- 비밀값: 원문 wrapper와 키 보유 객체에 Debug/Serialize가 없고, 토큰 실패는 고정 `InvalidAccess`로 변환한다. `AccessClaims`는 호출자가 필요한 정보를 이용하도록 Serialize를 제공하지만 Debug는 없다. RefreshHash는 DB 조회 값이므로 일반 동등 비교를 인증 비밀값 직접 비교의 결함으로 간주하지 않았다. SHA-256 입력은 원문 UTF-8 바이트이며 trim하지 않는다. (확인·추론)
- 미검증 범위: 전체 workspace 테스트는 이번 리뷰에서 재실행하지 않았다. 상단의 기존 전체 테스트 기록과 구분한다. OS 난수 공급 실패·실제 서명 장치 실패 주입, 성능 측정, 운영 롤백은 실행하지 않았다. 이번 변경에는 해당 실패를 만들기 위한 별도 대역을 필수로 요구할 근거가 없다. DB·로그인·HTTP·JWKS·7일 만료 및 폐기는 이번 리뷰 대상에 포함하지 않는다. (확인·추론)

리뷰를 위해 저장소에서 수정한 파일은 이 문서뿐이며, 기존 본문과 `AGENTS.md` 및 다른 사용자 변경을 보존한다. 코드·설정 수정, 커밋, 추가 subagent 실행은 하지 않았다. (확인)

## 토큰 파일 배치 변경

사용자 요청으로 `auth/token/mod.rs`를 `auth/token.rs`로 옮기고 토큰 테스트를 같은 파일의 `#[cfg(test)] mod tests`에 합쳤다. Rust 모듈 경로·공개 API·테스트 이름과 기존 동작은 유지한다. 과거 리뷰의 경로·해시는 당시 검토 대상을 나타낸다. (확인)

파일 통합 후 토큰 테스트 7개와 앱 전체 target clippy·fmt·diff 검사가 통과했다. (확인)

## 구현·변경 기록: AUTH-05 문서 시점 보완

사용자 요청으로 토큰 기반 최초 단계의 미연결 설명과 현재 저장·로그인·갱신·종료·API 연결 상태를 구분했다. 후속 문서 링크를 추가하고 과거 검증·리뷰는 유지했다. 문서·소스 대조와 diff 검사를 수행했으며 실행 코드와 기존 allow 사유는 변경하지 않았다. 실행 테스트는 다시 수행하지 않았다. (확인)

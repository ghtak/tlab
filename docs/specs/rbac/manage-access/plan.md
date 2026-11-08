---
id: PLN-RBAC-MANAGE-ACCESS
status: draft
---

# 역할·퍼미션 관리 설계 초안

## 연결 문서

| 관계 | 대상 | 설명 |
| --- | --- | --- |
| 참고 | [앱 개발 구조](../../../architecture/app-structure.md) | feature·slice 책임과 저장 계약 |
| 참고 | [사용자 추가](../../auth/create-local-user/plan.md) | 후속 RBAC 적용 대상 |
| 참고 | [JWT 정책](../../auth/jwt.md) | Access 유효기간·권한 변경 지연에 관한 기존 합의 |
| 참고 | [JWT API](../../auth/jwt-api/plan.md) | 인증과 업무 인가의 경계 |

## 현재 상태와 구조

- 계정의 식별자는 `tlab_account.id`이며 로그인 수단과 분리되어 있다. [스키마](../../../../tlab-app/migrations/0001_create_tlab_account.sql). 역할은 로그인 수단이 아닌 계정에 부여하는 것이 적절하다. (확인: 구조 / 추론: 부여 대상)
- 역할·퍼미션 테이블과 실제 인가 기능은 없다. [사용자 추가 route](../../../../tlab-app/src/auth/slice/create_local_user/route.rs)는 현재 인증 주체를 추출하지 않는다. (확인)
- [AccessClaims 추출기](../../../../tlab-app/src/auth/extractor.rs)는 JWT를 검증하고, 업무별 권한은 검사하지 않는다. [user_claim](../../../../tlab-app/src/auth/session_policy.rs)은 nickname·status만 제공한다. 토큰 테스트의 roles 예시는 실제 역할 기능이 아니다. (확인)
- 기존 JWT 문서는 권한 변경의 최대 10분 지연을 허용한다. 이번 설계는 사용자 지정에 따라 `user.role_ids`에 역할 ID를 넣고 역할별 퍼미션은 별도로 조회한다. 현재 발급 코드에는 아직 반영되지 않았다. (확인)

## 목표·범위

서비스 전체에서 공통으로 사용하는 역할과 퍼미션을 관리한다. 조직·워크스페이스에 따라 역할을 나누지 않는다. (확인: 사용자 지정)

역할 조회·생성·이름/설명 변경·삭제, 역할별 퍼미션 구성, 계정별 역할 부여·회수, 유효 퍼미션 조회와 업무 진입 인가를 초기 범위로 제안한다. 관리 화면의 구체적인 UI와 API 경로는 아직 정하지 않았다. (추론·제안)

계정별 직접 퍼미션, 역할 상속, 명시적 deny, 와일드카드, 리소스별 ACL은 필요가 확인되기 전까지 보류한다. 소유자만 수정할 수 있는 조건 등은 해당 업무가 소유하며 RBAC만으로 대체하지 않는다. (추론·제안)

## 설계안과 책임

```mermaid
flowchart LR
    A[계정] --> AR[계정별 역할 부여]
    AR --> R[역할]
    R --> RP[역할별 퍼미션 연결]
    RP --> P[퍼미션 카탈로그]
    H[업무 route 또는 usecase] --> S[RBAC 공용 권한 검사]
    AR --> I[로그인·갱신 시 JWT user.role_ids 구성]
    I --> H
    S --> T[RolePermissionResolver trait]
    T --> RP
```

| 주체 | 소유할 책임 |
| --- | --- |
| 업무 기능 | 필요한 퍼미션 코드·의미 정의, 자기 업무의 권한 검사 호출, 대상별 조건 판단 |
| RBAC feature | 퍼미션 카탈로그, 역할·연결·부여 데이터, 관리 정책과 유효 퍼미션 계산 |
| 인증 feature | 로그인·갱신 시 서버 역할 조회와 user 구성, 토큰 검증과 신뢰할 수 있는 계정·역할 식별 |
| 관리 권한을 가진 사용자 | 허용된 범위에서 역할 구성 및 계정별 부여·회수 |
| Container·HTTP 조립 | RBAC 의존성 및 Router 연결 |

이 책임 구분은 제안이다. 앱 고유 권한 정책은 `tlab-app`의 `rbac` feature가 소유하고 공통 라이브러리 `tlab`으로 이동하지 않는다. auth 토큰 검증기에 RBAC 저장 조회를 넣지 않는다. (추론·제안)

### 퍼미션 카탈로그와 역할

퍼미션은 업무 행위를 식별하는 고정 코드다. 이번 최소 카탈로그와 관리 권한 구분은 01에서 `account.create`, `rbac.read`, `rbac.role.manage`, `rbac.assignment.manage`로 구체화한다. (추론: 설계 선택)

퍼미션의 의미와 실제 검사 코드는 기능 개발 시 함께 정의한다. 카탈로그는 배포에 포함한 명시적인 migration/seed로 등록하고 일반 관리 API에서는 조회만 허용한다. 관리자가 임의 코드를 생성해도 실행 코드가 이를 검사하지 않으면 효과가 없으므로, 런타임에서 새 권한의 의미를 만드는 기능은 두지 않는 안이다. (추론·제안)

역할은 관리자가 퍼미션을 묶는 단위다. 계정은 여러 역할을 가질 수 있고 유효 퍼미션은 연결된 모든 역할의 합집합이다. 같은 퍼미션이 중복되어도 한 번만 취급하며, 역할 또는 퍼미션이 없으면 보호 작업을 거부한다. 역할 이름으로 업무 권한을 분기하지 않는다. (추론·제안)

### 데이터 모델

| 저장소 | 핵심 필드와 제약 |
| --- | --- |
| `tlab_rbac_permission` | `code` PK, 설명. 코드 변경·폐기는 코드와 배포 순서에 맞춰 처리 |
| `tlab_rbac_role` | 숫자 `id` PK, 고유 `name`, 설명, `version`, 생성·변경 시각 |
| `tlab_rbac_role_permission` | `(role_id, permission_code)` 복합 PK, 각각 FK |
| `tlab_rbac_account_role` | `(account_id, role_id)` 복합 PK, 각각 FK, 부여 시각 |

위 테이블과 필드는 제안이며 SQL은 작성하지 않았다. 계정 FK는 auth 소유 스키마에 의존한다. 역할 삭제 시 계정에 부여 중이면 충돌로 거부하고 먼저 회수하도록 한다. 역할 삭제 시 그 역할의 퍼미션 연결은 함께 제거한다. 사용 중인 퍼미션 카탈로그 삭제는 거부한다. 연결 테이블의 역방향 조회 인덱스는 실제 조회 SQL 확정 시 정한다. (추론·제안)

`version`은 역할 메타데이터·퍼미션 구성 변경의 동시 덮어쓰기를 막는 용도다. 계정별 역할 변경은 개별 부여·회수 연산으로 시작하며, 전체 목록 교체는 제공하지 않는 안이다. 부여 중복과 이미 없는 역할 회수는 성공으로 처리한다. (추론·제안)

### 앱 배치

`tlab-app/src/rbac`에 공용 entity·repository·권한 검사 service·route 등록을 두고, `slice` 아래에 역할 관리·퍼미션 구성·계정 역할 부여/회수 시나리오를 배치한다. 실제 파일 분리는 시나리오 크기에 맞춘다. repository는 저장 연산만 제공하고, 변경 정책과 트랜잭션은 usecase가 조합한다. (추론·제안)

업무 feature는 RBAC의 공용 권한 검사 계약만 사용한다. 로그인·갱신 slice나 RBAC 관리 slice의 내부에 의존하지 않는다. 기존 공용 오류 타입·HTTP 응답 구조를 확장해 인가 실패를 표현한다. (추론·제안)

## 공통 계약과 정책의 기준

상세 계약은 아래 [01 공통 계약·정책 확정](#rbac-01)에 모은다. 앞의 데이터 모델·앱 배치는 이 계약을 구현할 최소 구조이며, 02 이후 작업은 아직 실행하지 않는다. (확인: 이번 요청 / 추론: 설계 배치)

사용자가 확정한 정책은 서비스 전체 공통 RBAC, JWT `user.role_ids`, 역할→퍼미션 조회 trait과 최초 DB 구현·향후 cache 교체, 관리자도 보유 퍼미션까지만 위임이다. 추가 답변에 따라 초기 관리자 **새 계정을 운영 CLI로 생성**하고, 마지막 관리자·자기 회수를 따로 제한하지 않으며 필요하면 DB에서 직접 복구한다. 계정 상태는 로그인·갱신과 같은 정책을 사용하고, 감사는 별도 관리 예정이므로 제외한다. 더 강한 기존 역할의 수정·회수 제한은 적용하지 않는다. (확인: 사용자 합의)

문서는 구현 전 설계이므로 `draft`를 유지한다. 아래의 구체 타입·수치·파일 배치는 합의된 정책에서 도출한 이번 설계 선택이며 런타임 검증 결과가 아니다. (추론)

## 검증할 계약

- 역할이 없는 계정의 보호 작업 거부, 다중 역할 합집합, 같은 퍼미션을 주는 다른 역할이 남았을 때 회수 후 허용 유지. (추론)
- 로그인·명시적/웹 자동 갱신에서 최신 role_ids 발급, 역할 조회 실패 시 미발급, 기존 토큰에서 부여·회수가 즉시 반영되지 않는 계약 확인. (추론)
- resolver의 합집합·빈 입력·삭제 역할·조회 오류와 역할 퍼미션 변경의 기존 토큰 반영. (추론)
- 관리 권한이 없는 요청의 상태 변경 없음, 역할 편집과 부여를 통한 위임 제한 우회 거부. 보유 범위 내 부여·추가와 범위 밖 부여·추가 거부, 강한 기존 역할의 축소·회수 허용, 경합 중 최종 판정 확인. (추론)
- 실제 PostgreSQL 고유 테스트 스키마에서 FK·중복·원자성·version 충돌 및 정책에 필요한 경합 확인. (추론)
- 기존 사용자 추가 Router 검증에 401·403과 권한 보유 시 201을 연결하고, 기존 로그인·갱신·웹 쿠키 계약 유지 확인. (추론)

이번에는 설계 문서만 작성했으며 실행 검증은 수행하지 않았다. 문서 검토가 실제 권한 기능의 검증을 의미하지 않는다. (확인)

## 구현 순서와 서브 에이전트 작업 범위

각 항목을 하나의 서브 에이전트 작업으로 실행하는 안이다. 상위 에이전트는 공통 계약과 미결정 정책을 정리하고 작업 간 통합·문서 상태를 책임진다. 각 에이전트는 지정 범위와 관련 테스트만 수정하고, 다른 작업 내부를 가져다 쓰지 않는다. 사용자 요청은 실행 범위 정리이며 이 문서 갱신으로 구현을 시작하지 않는다. (확인: 요청 / 추론·제안: 작업 분할)

```mermaid
flowchart LR
    A[01 공통 계약·정책 확정] --> B[02 스키마·저장소]
    B --> C[03 DB resolver·인가]
    B --> D[04 JWT 발급 연결]
    C --> E[05 역할·퍼미션 관리]
    C --> F[06 계정 역할 관리·초기 관리자]
    D --> G[07 HTTP·Container·업무 연결]
    E --> G
    F --> G
    G --> H[08 통합 검증·문서 정리]
```

<a id="rbac-01"></a>
### 01. 공통 계약·정책 확정 — 선행 작업

**이번 실행 범위는 이 설계뿐이다.** 코드·migration·02 이후 구현·커밋·추가 에이전트 실행은 하지 않는다. 이전 작업 분할의 병렬 가능 표기는 후속 실행 시의 참고이며 실행 승인이 아니다. (확인: 사용자 지정)

#### 근거와 합의 경계

| 확인한 위치 | 설계에 반영할 사실 |
| --- | --- |
| [계정 모델](../../../../tlab-app/src/auth/entity.rs), [스키마](../../../../tlab-app/migrations/0001_create_tlab_account.sql) | 계정 ID는 i64/BIGINT. 상태는 REGISTERED·ACTIVE·WITHDRAWN |
| [session_policy](../../../../tlab-app/src/auth/session_policy.rs) | REGISTERED·ACTIVE 로그인 허용. user 구성은 현재 동기 함수이며 nickname·status만 포함 |
| [JWT](../../../../tlab-app/src/auth/token.rs), [추출기](../../../../tlab-app/src/auth/extractor.rs) | user는 JSON 객체. 검증기는 sub가 비어 있지 않음만 확인. RBAC의 계정 ID·역할 해석은 별도 필요 |
| [로그인](../../../../tlab-app/src/auth/slice/login_local/usecase.rs) | 계정 잠금→상태 재검사→Refresh INSERT→서명→commit. 반환은 commit 뒤 |
| [Refresh](../../../../tlab-app/src/auth/slice/refresh_access_token/usecase.rs), [웹](../../../../tlab-app/src/auth/web.rs) | 갱신은 현재 잠금 없는 조회. 웹도 동일 usecase 사용. InvalidRefreshToken만 쿠키 제거, 내부 장애는 제거하지 않음. 쿠키 전체 직렬화 한도 4,000 bytes |
| [계정 repository](../../../../tlab-app/src/auth/repository/account.rs), [DB context](../../../../tlab/src/sqlxdb.rs) | 호출자 소유 Context로 조회·잠금. 계정은 Refresh보다 먼저 잠금. commit 실패 시 결과 불명 가능 |
| [사용자 생성](../../../../tlab-app/src/auth/slice/create_local_user/usecase.rs), [실행 옵션](../../../../tlab-app/src/options.rs), [main](../../../../tlab-app/src/main.rs) | 가입 검증·해싱은 현재 slice 전용. 초기 관리자 CLI는 아직 없음 |
| [오류](../../../../tlab-app/src/app/error.rs), [응답](../../../../tlab-app/src/app/response.rs) | AppError가 공개 코드와 비공개 stage/cause를 분리. 신규 오류와 HTTP 매핑은 함께 추가해야 함 |

위 내용은 소스 확인이며 실행 결과를 뜻하지 않는다. (확인)

| 미합의였던 항목 | 검토한 최소안·이유 | 사용자 답변에 따른 결론 |
| --- | --- | --- |
| 초기 관리자·복구 | 기존 계정 승격보다 새 계정 생성 요구를 우선. 명시적 CLI는 첫 가입자 자동 승격과 기본 자격증명 노출을 피함 | 새 관리자 계정을 운영 CLI로 생성. 복구는 필요 시 DB 직접 처리 |
| 마지막 관리자·자기 회수 | 마지막 1명 보호는 관리자 정의와 전역 직렬화가 필요. 별도 보호 없이 운영 복구에 맡기는 안 제시 | 별도 관리·보호 없음. 자기 회수 허용, 마지막 관리 권한 소실도 차단하지 않음 |
| 계정 상태 | 기존 session_policy 재사용이 가장 작은 변경. ACTIVE 전용·매 요청 DB 확인은 별도 정책 변경 | 로그인·갱신과 같은 정책. 현재 REGISTERED·ACTIVE 허용 |
| 감사 | 성공 변경을 같은 트랜잭션에 기록하는 안과 별도 관리안 비교 | 영속 감사 제외. 감사 테이블·필수 감사 이벤트·감사 실패 rollback 계약을 추가하지 않음 |
| 강한 기존 역할 제한 | 부여/추가만 보유 범위 검사하면 위임 제한 충족. 변경 전·후 전체 범위를 요구하면 긴급 회수·축소까지 막음 | 강한 제한 미적용. 역할 회수·권한 제거·메타데이터 변경·삭제에 대상 전체의 보유 조건 없음 |

근거와 최소안은 먼저 질문했고 사용자 답변을 반영했다. 감사 제외는 기존 오류 진단 로깅 제거를 뜻하지 않는다. 마지막 관리자 보호가 없으므로 이를 위한 전역 잠금·관리자 수 집계도 두지 않는다. (확인: 답변 / 추론: 설계 영향)

#### 공개 타입과 소유권

앱 정책은 `tlab-app::rbac`이 소유한다. `tlab`·일반 JWT 검증기는 RBAC를 알지 않는다. feature 내부 구현은 비공개로 하고 `rbac/mod.rs`에서 다른 feature·조립이 사용할 계약만 `pub(crate)`로 내보낸다. (추론: 설계 선택)

```rust
// rbac/entity.rs: 필드는 비공개, 검증된 생성자와 읽기 메서드만 공개
struct RoleId(i64);                  // Copy + Eq + Ord + Hash, 1..=i64::MAX
struct RoleIds(Vec<RoleId>);         // 오름차순·중복 제거
struct PermissionCode(String);      // 고정 카탈로그 코드
struct PermissionSet(BTreeSet<PermissionCode>);
struct Actor { account_id: i64, role_ids: RoleIds }

// 표준 Future/Pin. 외부 async-trait 의존성은 추가하지 않음
trait RolePermissionResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        role_ids: &'a RoleIds,
    ) -> Pin<Box<dyn Future<Output = tlab::Result<PermissionSet>> + Send + 'a>>;
}
```

코드 블록은 구현할 Rust 계약의 요약이다. `Future`, `Pin`, `BTreeSet`은 std 타입이며 `Arc<dyn RolePermissionResolver>`로 보관할 수 있게 객체 안전성을 유지한다. native `async fn`/제네릭 메서드/연관 Future를 이 dyn 계약에 넣지 않는다. (추론: 설계 선택)

- `RoleId::try_from(i64)`와 `get() -> i64`, `RoleIds::try_from_raw(Vec<i64>)`와 `as_slice() -> &[RoleId]`를 제공한다. 각 ID를 검증하고 정렬·중복 제거한다. `RoleId` JSON 표현은 양수 정수이며 문자열·실수는 허용하지 않는다. bigint ID는 JS number 안전 범위를 넘을 수 있으므로 UI가 숫자 산술·재인코딩하지 않도록 후속 API/UI 계약에서 다뤄야 한다. (추론)
- `PermissionCode::try_from(&str)`는 1~64 ASCII bytes, 소문자 영문으로 시작하는 점 구분 segment(`[a-z][a-z0-9_]*`)만 받는다. trim·대소문자 변환·와일드카드는 없다. 형식 검증과 카탈로그 존재 검사는 별개다. 의미는 업무 기능이 정의하고 등록은 배포로 수행한다. (추론)
- `PermissionSet`은 읽기·`contains`·`is_subset`·합집합·차집합을 제공하며 순서는 코드 사전순이다. 빈 집합은 권한 없음. 직접 권한·상속·deny·admin 우회는 없다. 외부에서 무검증 String 집합을 넣어 검사하지 않는다. (추론)
- `Actor::try_from_verified_claims(&AccessClaims) -> tlab::Result<Actor>`는 RBAC claims 계층이 제공한다. sub는 선행 0 없는 양수 십진 문자열(`1..=i64::MAX`)만 허용한다. 검증된 JWT의 계정 ID 외에 body/query의 actor ID를 신뢰하지 않는다. (추론)

#### 조회·claims·크기·오류 계약

`DbRolePermissionResolver`는 Database 핸들을 소유하고 호출마다 연결을 얻어 역할 집합으로 일괄 조회한다. N+1 조회를 하지 않는다. 빈 RoleIds는 DB 연결 없이 빈 집합을 반환한다. 삭제·미존재 역할은 기여하는 퍼미션이 없고 나머지 역할은 계속 합산한다. DB 오류는 `tlab::Error::DbError`, 유효하지 않은 저장 값은 `tlab::Error::IllegalState`이며 부분 집합으로 성공시키지 않는다. 캐시는 이번에 구현하지 않는다. 향후 동일 trait을 구현하되 TTL·무효화·다중 인스턴스 일관성·장애 및 stale 허용 여부를 먼저 명세한다. (확인: 교체 경계 / 추론: 세부 계약)

계정 역할 조회는 DB 연결을 내부에서 새로 얻는 resolver와 분리한다. 공용 repository API는 다음과 같다. 계정 존재와 상태는 auth가 판단하고 이 조회는 연결만 반환한다. 따라서 미존재 계정의 빈 결과를 계정 존재 증거로 사용하지 않는다. (추론)

```rust
// rbac/repository/account_role.rs
async fn list_role_ids(
    ctx: &mut crate::app::db::Context<'_>, account_id: i64,
) -> tlab::Result<RoleIds>;

// rbac/repository/role_permission.rs: 관리 tx에서도 같은 조회 사용
async fn resolve_in_context(
    ctx: &mut crate::app::db::Context<'_>, role_ids: &RoleIds,
) -> tlab::Result<PermissionSet>;
```

인증 feature는 `rbac`의 공개 재노출 함수를 사용하며 private repository 경로에 의존하지 않는다. DB context의 lifetime은 기존 API를 그대로 따른다. 계정 조회용으로 별도 dyn trait은 만들지 않는다. 발급 트랜잭션 안에서 같은 연결을 전달할 수 있어야 하기 때문이다. (추론)

| 입력·한도 | 이번 설계의 처리 |
| --- | --- |
| `user.role_ids` 누락 | 구형 토큰 호환으로 빈 역할. 정상 인증이지만 보호 작업은 403 |
| `[]`, 중복·순서 다른 유효 정수 | 빈 역할 또는 정규화된 역할 목록 |
| null·배열 아닌 값·문자열/실수/음수/0/범위 초과 원소 | claims 입력 검증 실패 → 401. 일부 정상 원소만 채택하지 않음 |
| 웹 쿠키 | 기존 속성 포함 4,000 bytes 검사를 그대로 적용. 이 전달 계층의 실제 한도만 유지하고 별도 JWT 3,000 bytes 한도는 추가하지 않음 |

```rust
// rbac/authorization.rs: 모든 업무의 최소 검사, DB/cache 구분 없음
async fn require_permission(
    actor: &Actor, required: &PermissionCode,
    resolver: &dyn RolePermissionResolver,
) -> tlab::Result<bool>; // true 허용, false 거부, Err 조회 실패
```

오류 타입은 기존 `tlab::Error`/`tlab::Result`만 사용한다. 타입 생성의 잘못된 입력은 `InvalidInput`, 저장 값의 형식 위반은 `IllegalState`, SQL 실패는 원래 `DbError(sqlx::Error)`를 그대로 반환한다. RBAC 전용 InputError·LookupError·ManagementError·AuthorizationError는 만들지 않는다. 정책 거부는 정상적인 판정 결과이며, 세분화된 저장소 오류로 만들지 않는다. (확인: 사용자 수정 요청 / 추론: 책임 경계)

HTTP 변환은 후속 usecase/route가 실패한 맥락에서 담당한다. 인증/claims 실패 401, 권한 거부 403, 잘못된 입력 400, 권한 확인 후 대상 부재 404, version·이름·부여 중 삭제 충돌 409, 조회/내부 실패 500이라는 응답 의미는 유지한다. 이를 위해 02 저장소에 HTTP/업무별 오류 enum을 미리 추가하지 않는다. 예상 DB 충돌을 구분해야 할 때는 원래 sqlx 오류의 SQLSTATE·constraint를 필요한 경계에서 확인한다. 문자열 메시지로 분기하지 않는다. (추론)

deadlock·연결 실패·commit 실패에 자동 재시도를 넣지 않는다. commit 실패는 결과 불명일 수 있어 “항상 적용되지 않음”을 보장하지 않는다. (추론)

#### 관리 정책과 함수 계약

관리 퍼미션은 최소 3개로 묶는다. `rbac.read`는 카탈로그·역할·타인 계정 역할/유효 퍼미션 조회, `rbac.role.manage`는 역할 생성·메타데이터·구성·삭제, `rbac.assignment.manage`는 계정 역할 부여·회수다. 본인의 역할/유효 퍼미션 조회는 인증만 요구한다. `account.create`는 사용자 생성 업무의 진입 권한이다. 이 네 코드는 후속 카탈로그에 명시 등록하며 관리자 역할 이름에는 의미를 부여하지 않는다. (추론: 최소 권한 구분)

| 변경 | 필수 퍼미션 | 보유 범위 검사 |
| --- | --- | --- |
| 빈 역할 생성·메타데이터 변경 | rbac.role.manage | 추가 위임이 없으므로 대상 역할 전체와 비교하지 않음 |
| 역할 퍼미션 교체 | rbac.role.manage | `(after − before) ⊆ actor_permissions`만 요구. 기존에 더 강한 권한이 남거나 제거되어도 허용 |
| 역할 삭제 | rbac.role.manage | 대상 전체 보유 조건 없음. 부여 중 삭제 금지·version은 별도 적용 |
| 계정 역할 부여 | rbac.assignment.manage | 대상 역할의 현재 전체 퍼미션이 actor_permissions 부분집합이어야 함 |
| 계정 역할 회수 | rbac.assignment.manage | 대상 전체 보유 조건 없음. 자신/마지막 관리자도 별도 보호 없음 |

정책 함수는 DB 접근 없는 순수 bool 판정이며, 업무 usecase가 false를 인가 거부로 처리한다. 저장소 조회 실패와 정상 거부를 혼동하지 않는다. (추론)

```rust
fn require_in_set(
    actor_permissions: &PermissionSet, required: &PermissionCode,
) -> bool;
fn check_role_edit(
    actor_permissions: &PermissionSet, before: &PermissionSet, after: &PermissionSet,
) -> bool;
fn check_assignment_grant(
    actor_permissions: &PermissionSet, target_permissions: &PermissionSet,
) -> bool;
```

03이 이 공통 정책을 소유하고 05·06은 호출만 한다. 회수·삭제가 권한 보유 범위의 비교를 우회하는 특별 관리자 기능인 것은 아니며, 해당 관리 퍼미션은 항상 필요하다. (추론)

역할 생성은 빈 구성으로 시작한다. 구성 교체는 기대 version을 필수로 받고 성공 시 version을 1 증가시킨다. 이름·설명 변경도 같은 규칙이다. 동시 덮어쓰기는 409다. 부여/회수는 목록 전체 교체 대신 단건 API이며, 중복 부여·없는 연결 회수는 동일한 관리 권한 확인 후 성공한다. 대상 계정·역할 자체가 없으면 404다. 변경 시각·version의 no-op 갱신은 하지 않는다. (추론)

자기 역할/유효 퍼미션 조회도 표시할 대상 계정의 현재 DB 연결을 조회한다. 이 결과는 현재 저장 상태이며 기존 Access에 남아 있는 RoleIds의 권한과 다를 수 있다. 실제 요청 인가는 언제나 그 요청의 Actor를 사용한다. 타인 조회는 rbac.read 검사 후 계정 존재를 확인한다. (추론)

행위자의 계정 역할 목록은 검증된 JWT를 사용한다. 일반 인가·관리 쓰기 모두 계정→역할을 재조회하여 기존 토큰을 즉시 무효화하지 않는다. 역할 퍼미션은 현재 DB를 사용한다. 따라서 계정 역할 회수 후에도 기존 Access는 최대 600초 동안 관리 근거가 될 수 있고, 역할별 퍼미션 제거는 커밋 후 새 조회부터 반영된다. 처리 중 요청의 취소·역할 편집이 이미 완료된 업무 부수 효과를 되돌리는 보장은 없다. 삭제 RoleId는 재사용하지 않는다. (확인: 기존 지연 정책 / 추론: 구조에 따른 결과)

상태 판정은 auth의 `session_policy::allowed(&Account)`를 공개 공용 함수로 재사용한다. 현재 REGISTERED·ACTIVE는 허용하고 WITHDRAWN은 발급/새 역할 부여를 거부한다. 회수는 WITHDRAWN에도 허용해 정리를 막지 않는다. 일반 요청은 상태 DB 재조회 없이 기존 Access 유효기간 정책을 따른다. JWT의 status 문자열을 별도 RBAC 활성화 규칙으로 쓰지 않는다. 관리 tx에서는 행위자 계정의 현재 allowed와 대상 계정 부여 가능 여부를 확인한다. 로그인·갱신 상태 정책 자체를 바꾸거나 계정 상태 변경 기능을 이 작업에 추가하지 않는다. (확인: 동일 상태 정책 / 추론: 적용 경계)

#### 잠금 순서와 트랜잭션 경계

요청 진입 인가는 resolver를 사용하지만 관리 쓰기의 최종 판정은 **같은 DB 트랜잭션**에서 한다. cache/별도 연결의 PermissionSet으로 변경을 승인하지 않는다. repository는 저장·잠금 API만 제공하고 05·06 usecase가 tx 생명주기를 소유한다. (추론)

1. tx 시작. 기존 계정 행은 필요한 전체 ID(행위자·부여/회수 대상)를 합쳐 오름차순 `FOR UPDATE`로 잠근다. 같은 계정은 한 번만 잠근다. 계정 부재·상태를 확인한다.
2. JWT 행위자 RoleIds와 대상 RoleId의 합집합을 오름차순으로 한 행씩 잠근다. 역할 목록이 아닌 이 정렬 규약을 모든 역할 변경·부여·회수·삭제 경로가 따른다. 부재한 행을 잠글 수 없으므로 존재 여부는 별도로 구분한다. 오래된 JWT의 삭제 역할은 무권한으로 처리한다.
3. 역할 잠금이 모두 확보된 다음 statement에서 `resolve_in_context`로 행위자 현재 퍼미션과 대상 구성을 다시 읽고 관리 권한·위임 범위·version·부여 중 삭제 금지를 확인한다. PostgreSQL READ COMMITTED에서 잠금 대기 전 읽은 값을 재사용하지 않는다.
4. 연결/역할 변경을 원자적으로 수행하고 commit 후 성공을 반환한다. 실패·취소는 미완료 tx rollback을 따르고 토큰·완료 응답을 내보내지 않는다. 외부 호출·해싱·cache 호출을 잠금 안에 넣지 않는다.

같은 역할을 수정하는 05와 부여/회수하는 06은 역할 행 잠금으로 직렬화된다. JWT 로그인 역할 조회의 직렬화는 계정 행 잠금으로 보장한다. 역할 삭제도 이 규약에 참여하므로 FK만으로 경합을 해결하지 않는다. 퍼미션 카탈로그는 일반 관리 쓰기로 수정하지 않는다. (추론)

기존 auth의 **계정→Refresh** 순서를 유지한다. 여러 종류를 모두 다루는 새 경로는 **계정(ID 오름차순)→역할(ID 오름차순)→Refresh** 순서다. 역할을 잡은 뒤 계정 잠금을 추가 획득하지 않는다. 역할 관리가 사용자 전체 계정 행을 잠그는 방식은 사용하지 않는다. 계정 상태 변경·물리 삭제에서 RBAC 변경까지 조합하는 후속 경로도 이 순서를 지켜야 한다. 직접 운영 DB 조치는 이 API의 동시성 보장 밖이며 운영자가 정합성을 책임진다. (추론)

```mermaid
sequenceDiagram
    participant U as 관리 usecase
    participant T as 같은 DB tx
    U->>T: 계정 정렬 잠금 → 역할 정렬 잠금
    U->>T: 현재 퍼미션·구성·version 재조회
    U->>U: 관리 권한 + 추가/부여 범위 판정
    U->>T: 변경
    T-->>U: commit
    U-->>U: 성공 응답 생성
```

일반 조회 resolver는 한 SELECT의 일관된 합집합을 제공한다. 로그인은 기존 계정 잠금을 유지하고 `list_role_ids`를 같은 tx context로 호출한다. Refresh는 기존 잠금 없는 경로를 유지하므로 부여/회수와 경합한 조회는 변경 전 또는 후 역할 집합을 발급할 수 있다. 각 집합은 한 SELECT로 읽으며 커밋 후 시작한 조회는 새 집합을 본다. Refresh를 잠금 tx로 확대하지 않는다. 마지막 관리자 보호·감사 저장용 트랜잭션 확장은 없다. (추론)

#### 발급·실패 계약

`auth::session_policy::user_claim(account: &Account, role_ids: &RoleIds) -> Map<String, Value>`로 바꾸되 동기 직렬화만 수행한다. role_ids는 서버 조회 결과만 받으며 정렬된 JSON 정수 배열을 포함한다. 빈 결과는 `[]`다. auth 로그인·명시적 Refresh·웹 자동 Refresh는 동일 발급 구성을 사용하고 일반 토큰 검증기에 DB 조회를 추가하지 않는다. (확인: JWT 위치 / 추론: 연결 계약)

- 로그인: 입력 검증·해싱/검증 후 tx→계정 잠금→상태 검사→역할 조회→Refresh INSERT→user 구성·Access 서명→commit→응답. 조회 실패는 Refresh INSERT 이전에 종료한다. 이후 실패도 같은 tx를 rollback하고 토큰을 반환하지 않는다. 내부에서 만든 토큰 문자열은 commit 전에 외부로 공개하지 않는다. (추론)
- Refresh: 기존 저장 세션·계정·상태/만료 검증→같은 연결에서 역할 조회→구성·서명→반환. 실패 시 기존 Refresh를 폐기·교체하거나 만료를 연장하지 않는다. 클라이언트가 보낸 역할·만료 Access·sid로 발급 근거를 대체하지 않는다. (추론)
- 웹 자동 갱신: 공통 Refresh usecase의 역할 조회 실패는 500/no-store, 업무 handler 미실행, 새 Access cookie 없음, 기존 쿠키 유지. 이를 InvalidRefreshToken으로 바꿔 로그인 해제하지 않는다. 무효/만료 Refresh의 기존 401·쿠키 제거 계약은 유지한다. (확인: 기존 웹 분기 / 추론: 신규 장애 적용)
- 웹의 최종 쿠키 직렬화 실패는 기존 `AUTH_COOKIE_TOO_LARGE` 응답을 유지한다. 이 검사는 로그인 tx commit 뒤일 수 있어 새 Refresh 저장까지 되돌린다고 보장하지 않는다. RBAC 역할 조회·서명 실패는 commit 전 처리하고, 전달 계층 실패와 구분한다. (추론)

#### 초기 관리자 운영 CLI

신뢰된 운영자가 명시 실행하는 CLI에서 **새 로컬 계정**을 만든다. 기존 일반 사용자 생성 route를 호출하거나 첫 가입자를 자동 승격하지 않는다. 이메일·닉네임·비밀번호는 명시 입력하며 기본 자격증명은 없다. 비밀번호는 터미널 비표시 입력 또는 stdin으로 받고 argv/로그·출력에 노출하지 않는다. 현재 카탈로그의 모든 퍼미션을 가진 보통 역할을 만들고 그 계정에 부여한다. 향후 추가되는 퍼미션을 자동 포함하거나 admin 이름으로 우회하지 않는다. (확인: 사용자 선택·질문 최소안 / 추론: 입력 전달)

06은 `rbac/slice/bootstrap_admin` usecase를 소유한다. 기존 가입 slice 내부를 가져오지 않도록 auth가 소유하는 공용 로컬 계정 준비 작업(정리·검증·비밀번호 정책·해싱)을 추출해 일반 가입과 초기 관리자 생성이 함께 사용한다. 이 공용 작업은 tx를 시작하지 않고 비밀을 포함하므로 Debug/Serialize 없는 `PreparedLocalAccount`를 반환한다. auth의 `insert_prepared_local_account(ctx, prepared) -> Result<Account, AppError>`가 같은 Context로 account/identity를 저장한다. 초기 생성 usecase는 이 공개 작업과 role/role_permission/account_role 저장을 **한 tx**로 조합하며 auth private repository에 직접 의존하지 않는다. 초기 계정 상태는 기존 가입과 같은 REGISTERED다. 비밀번호 해싱은 tx 전에 끝낸다. (추론)

CLI는 마이그레이션이 끝난 카탈로그를 전제로 한다. 최초 생성만 허용하는 DB 트랜잭션 advisory lock을 초기화 전용 고정 키로 획득하고, RBAC 역할이 이미 존재하거나 입력 이메일이 중복이면 충돌 종료한다. 동시 CLI 중 하나만 초기화하고 계정·역할을 덮어쓰거나 추가 관리자를 계속 만들지 않는다. 카탈로그가 비어 있거나 필수 관리 코드가 없으면 실패한다. 실패 시 새 계정·로그인 수단·역할·연결 모두 rollback한다. 성공 출력은 계정 ID·역할 ID뿐이며 토큰 발급·서버 시작을 하지 않는다. 복구용 별도 우회 API/CLI는 추가하지 않고 필요 시 DB 직접 복구한다. (추론: 최초 생성·반복 실행의 최소 계약)

초기화 advisory lock은 일반 관리 전역 잠금이 아니다. 배포 전 최초 초기화에만 사용하고 이미 역할이 있는 서비스에는 동작하지 않는다. 후속 CLI 등록 이름은 `bootstrap-admin`으로 하며 기존 `--migrate`/서버 실행과 상호 배타적으로 선택한다. (추론)

#### 후속 작업 파일 소유권과 인계

01은 이 plan만 수정한다. 아래는 **향후 구현의 단독 파일 소유권**이며 현재 파일 생성·작업 실행은 없다. 신규 루트 모듈·enum 선언을 여러 작업이 동시에 편집하지 않는다. (확인: 범위 / 추론: 배치)

| 작업 | 단독 수정 파일·경계 | 공개 인계 |
| --- | --- | --- |
| 02 | `rbac/mod.rs`, `entity.rs`, `repository/mod.rs`, `repository/{role,permission,role_permission,account_role}.rs`, 새 번호 SQL, 필요 시 `src/migrations.rs` | 위 타입·읽기/잠금 CRUD 구현. rbac 루트에서 `mod resolver; mod claims; mod authorization; mod policy; mod slice; mod route;` 선언과 공개 재노출도 단독 관리. 후속 파일 준비 전 중간 상태를 통합 완료로 간주하지 않음 |
| 03 | `rbac/resolver.rs`, `claims.rs`, `authorization.rs`, `policy.rs`, 해당 모듈 테스트 | DbRolePermissionResolver·Actor 해석·require_permission·순수 관리 정책. 02에 선언/재노출 변경을 요청하고 mod.rs 직접 수정하지 않음 |
| 04 | `auth/session_policy.rs`, `auth/slice/login_local/`, `auth/slice/refresh_access_token/`, 발급 관련 기존 테스트 | 명시 RoleIds user 구성·같은 context 조회·기존 웹 쿠키 전달 검사. 일반 token 검증 규칙은 유지. 공개 함수는 07이 auth/mod.rs에서 재노출 |
| 05 | `rbac/slice/{list_permissions,list_roles,create_role,update_role,replace_role_permissions,delete_role}/`와 테스트 | 역할·카탈로그 usecase/handler. slice/mod.rs·route.rs는 07에 목록 인계 |
| 06 | `rbac/slice/{list_account_roles,grant_account_role,revoke_account_role,get_effective_permissions,bootstrap_admin}/`, `auth/local_account.rs`, `auth/slice/create_local_user/usecase.rs`와 해당 테스트 | 계정 역할·CLI 초기화 usecase, auth 소유 가입 준비 공용화. 인증 결과/상태 정책을 중복 정의하지 않음. auth 준비/저장 facade의 공개 재노출은 07에 인계 |
| 07 | `src/main.rs`, `options.rs`, `app.rs`, `http.rs`, `app/{error,response}.rs`, `auth/{mod,route}.rs`, `auth/slice/create_local_user/route.rs`, `rbac/{route.rs,slice/mod.rs}` | `mod rbac;` 루트 선언·Container Arc 주입·Router·오류 매핑·CLI 등록. auth 공개 경계와 사용자 생성 진입 인가 담당 |
| 08 | 통합 테스트·관련 verification.md·이 plan 하단 결과 기록 | 통합 검증. 계약 수정은 원 소유 작업으로 돌려보내며 미승인 정책/기능을 추가하지 않음 |

04와 06이 auth 공용 선언을 서로 편집하지 않도록 07이 일괄 조립한다. 02~06 개별 산출물은 인계 단위이며 07 이전 각 중간 상태의 전체 앱 컴파일 성공을 전제하지 않는다. 공개 계약 변경 필요 시 해당 소유 작업을 통해 plan과 소비자를 함께 조정한다. 이 소유권 표가 아래 작업별 담당 설명보다 구체적인 기준이다. (추론)

#### 01 완료 판단과 후속 검증 조건

- 정책 5개에 대한 사용자 답변을 반영했고 별도 핵심 정책 질문은 남지 않았다. 타입·trait·조회·실패·잠금·발급·파일 경계를 이 설계로 인계할 수 있다. **01 설계 범위는 완료, 구현·실행 검증은 미수행**이다. draft는 실제 구현·검증 완료를 뜻하지 않는다. (확인·추론)
- 03: dyn Arc 컴파일, 빈/중복/삭제 역할·DB 실패·invalid claims와 합집합 검증. 05·06: 부여/추가 범위 초과 거부, 강한 기존 역할의 제거·회수·이름 변경 허용, 자기 회수/마지막 관리자 허용을 확인한다. (추론: 후속 조건)
- PostgreSQL 격리 스키마에서 역할 편집↔부여/회수·역할 삭제↔부여을 검증한다. 마지막 관리자 전역 경합 보호·감사 저장 검증은 요구하지 않는다. (추론)
- 04·07: 로그인 역할 조회 실패 시 Refresh 미저장, Refresh 장애 시 기존 세션·쿠키 유지와 handler 미실행, 모든 발급 경로의 role_ids 일치을 확인한다. CLI 실패 원자성·재실행/동시 초기화 충돌·비밀 미출력도 검증한다. (추론)

### 02. RBAC 스키마·entity·repository

- **선행:** 01 완료.
- **담당:** 새 번호 migration, `rbac/entity`와 `rbac/repository`, migration 등록에 필요한 최소 변경. 기존 앱 migration 실행 방식을 확인해 재실행 계약을 유지한다.
- **산출물:** 네 테이블·제약·필요 인덱스, 역할 및 연결 CRUD, 계정 역할 조회, 역할별 퍼미션 일괄 조회, 역할 잠금과 version 조건부 갱신 API. 감사 저장소는 이번 범위에서 제외한다.
- **완료 기준:** 고유 PostgreSQL 스키마에서 제약·중복·조회 결과·트랜잭션 rollback 검증. 정책 판단을 repository에 넣지 않는다. 후속 작업은 저장소 구현 대신 이 API를 사용한다. (추론·제안)

### 03. DB resolver·공용 인가

- **선행:** 01·02 완료. 04와 병렬 가능.
- **담당:** `rbac`의 resolver DB 구현, claims 해석과 공용 권한 검사·위임 정책 모듈 및 해당 테스트.
- **산출물:** 역할 퍼미션 합집합, 허용/거부/조회 실패 구분, 입력·오류 계약. 캐시 구현은 포함하지 않는다.
- **완료 기준:** 빈 목록·중복·삭제 역할·DB 장애, 필요한 퍼미션 보유/미보유, 보유 범위 비교 검증. 공용 모듈 선언은 01에서 지정된 파일에만 반영한다. (추론·제안)

### 04. 로그인·갱신의 JWT role_ids 발급

- **선행:** 01·02 완료. 03과 병렬 가능.
- **담당:** auth의 `session_policy`, 로그인·Refresh 발급 로직과 관련 기존 테스트. Container·공용 HTTP 조립 파일은 07이 담당한다.
- **산출물:** 서버 계정 역할 조회 후 동일한 user 구성, 로그인·명시적 Refresh·웹 자동 Refresh에 반영. 토큰 서명·일반 검증기의 책임은 유지한다.
- **완료 기준:** 빈/다중 역할, 부여·회수 후 다음 발급 반영, 역할 조회 실패 시 미발급 및 로그인 트랜잭션 rollback, 기존 Refresh·쿠키 계약 검증. 웹 코드가 기존 Refresh usecase를 호출해 해결되면 중복 구현하지 않는다. (추론·제안)

### 05. 역할·퍼미션 관리 slice

- **선행:** 02·03 완료. 06과 병렬 가능.
- **담당:** 퍼미션 카탈로그 조회, 역할 조회·생성·수정·삭제와 퍼미션 구성 slice, 각 handler·usecase·테스트. Router 등록은 07 담당.
- **산출물:** 관리 행위 권한 + 위임 한도 검사, 역할 잠금·version 충돌·부여 중 삭제 제한, 감사 기록은 제외한다.
- **완료 기준:** 보유 범위 안 추가 성공, 범위 밖 퍼미션 추가·간접 상승 거부, 기존 강한 권한 제거 허용, 원자성·동시 변경 충돌과 오류 변환 검증. 06의 내부 코드를 참조하지 않는다. (추론·제안)

### 06. 계정 역할 부여·회수와 초기 관리자

- **선행:** 01·02·03 완료. 05와 병렬 가능.
- **담당:** 계정 역할 조회·부여·회수 slice·handler·테스트, 합의된 초기 관리자 설정 작업. 초기화 진입점 추가가 필요하면 등록은 07에 전달한다.
- **산출물:** 위임 한도, 중복 부여/회수, 계정·역할 부재, 자기 회수·마지막 관리자 별도 보호 없음 정책, 초기 관리자 부여·복구 계약. 초기화는 일반 사용자가 호출하는 우회 경로로 노출하지 않는다.
- **완료 기준:** 권한 초과 부여 거부·관리 권한에 따른 회수 허용, 잠금 경합과 정책 불변 조건, 초기화의 반복 실행·실패 처리 검증. 05와 공통인 정책은 03 공개 계약을 사용한다. (추론·제안)

### 07. Container·Router·기존 보호 작업 통합

- **선행:** 03~06 완료.
- **담당:** `app::Container`, `http`, auth/RBAC Router 등록, 공용 HTTP 오류 매핑, 사용자 추가 route, 필요 시 초기화 CLI 등록. 공용 조립 파일은 이 작업에서 단독 수정한다.
- **산출물:** `Arc<dyn RolePermissionResolver>`에 DB 구현 주입, 관리 API 연결, 사용자 추가의 인증·퍼미션 확인, 합의한 경로·상태 코드 연결.
- **완료 기준:** 미인증 401·권한 없음 403·허용 시 사용자 추가 201, 조회 장애 5xx, 변경 실패 시 부수 효과 없음. JSON API와 웹 쿠키·CSRF 연결의 대표 경로를 검증한다. (추론·제안)

### 08. 통합 검증·설계 대조

- **선행:** 07 완료.
- **담당:** 통합 검증과 기존 테스트 보완, 이 plan의 구현·변경 기록 및 필요 시 작업별 verification 문서. 계약 결함 발견 시 해당 소유 작업에 수정 사항을 돌려보낸다.
- **완료 기준:** 실제 PostgreSQL 격리 환경에서 발급→인가→역할 변경→갱신 흐름, 기존 JWT에 남은 역할과 현재 퍼미션 조회의 차이, 관리 권한 상승·역할 편집/부여 경합 검증. 관련 workspace 테스트·fmt·clippy를 수행하고 실제 실행 범위와 미검증 항목을 기록한다.
- **상위 에이전트 마무리:** 결과 diff와 공개 계약·문서 링크·범위 확인. 문서 채택 상태는 사용자 합의 근거에 따라 갱신하며 테스트 통과만으로 accepted로 바꾸지 않는다. (추론·제안)

병렬 그룹은 **03·04**, **05·06**이다. 공용 파일 충돌이나 공개 계약 변경이 발견되면 상위 에이전트가 먼저 조정하고 후속 작업에 전달한다. 구현 중 추가된 기록은 앞선 기록을 보존하고 이 문서 하단에 발생 순서대로 이어 쓴다. (추론·제안)

## 구현·변경 기록

### 초기 설계 초안

- 사용자 요청과 기존 계정·JWT·사용자 추가 코드를 근거로 책임·모델·인터페이스 초안을 작성했다. (확인)
- 서비스 전체 공통 범위를 반영했다. 나머지 정책은 제안과 미결정으로 구분했다. (확인)
- 앱 코드·migration은 변경하지 않았으며 구현 전 핵심 정책 논의가 남아 있다. (확인)

### 사용자 지정 반영: JWT 역할과 조회 교체 경계

- JWT `user.role_ids`, 역할→퍼미션 trait과 최초 DB 구현·향후 캐시 교체, 관리자 보유 퍼미션 한도라는 사용자 지정을 반영했다. (확인)
- 계정 역할과 역할별 퍼미션의 반영 시점, 로그인·갱신 연결, 관리 쓰기에서의 일관성 검사와 조회 trait의 책임을 구체화했다. 세부안은 제안이며 전체 문서는 draft를 유지한다. (추론·제안)
- 사용자 요청에 따라 아래에 서브 에이전트별 구현 순서를 정리했다. 아직 구현 에이전트를 실행한 것은 아니다. (확인)

### 01 공통 계약·정책 정리 (설계만)

- AGENTS.md·create-plan 및 관련 가이드, 계정·JWT·로그인·Refresh·웹·DB·오류·CLI 코드를 확인했다. docs/notes는 읽지 않았다. (확인)
- 미합의 5개 항목의 근거·최소안을 질문한 뒤 사용자 답변을 반영했다. 초기 관리자 새 계정의 운영 CLI 생성까지 확인했다. 초기 관리자 보통 역할·반복 실행·입력 전달 세부는 01에 설계 선택으로 명시했다. (확인·추론)
- 01에 공개 타입·객체 안전 비동기 trait·context 조회·claims/오류/한도·최소 위임 검사·잠금/tx·발급 실패·후속 파일 소유권을 구체화했다. 기존 변경 전/후 전체 범위 제한은 제거했다. (확인)
- 문서 내용·내부 링크·변경 범위를 점검했다. 실행 테스트는 문서만 변경한 범위에서 수행하지 않았다. 런타임 코드·migration·02 이후 구현·커밋·추가 에이전트 실행은 하지 않았다. 이 문서는 draft이며 01 설계 산출물과 구현 완료를 구분한다. (확인)

### 02 RBAC 스키마·entity·repository 구현

사용자가 02의 코드·migration 변경과 검증·자체 점검까지 명시 승인했다. 이전의 설계만 수행 제한은 02에 한해 해제했고, 03 이후 구현·추가 에이전트·커밋·배포는 진행하지 않았다. 기존 정책과 공개 계약의 실질 변경은 없다. (확인)

#### 변경 내용과 통합 전 검증 경로

- [0003_create_tlab_rbac.sql](../../../../tlab-app/migrations/0003_create_tlab_rbac.sql)에 네 테이블, 양수 ID/version·역할명·퍼미션 코드 제약, 복합 PK·FK·역방향 인덱스와 카탈로그 네 코드를 추가했다. 계정 삭제는 부여 연결 cascade, 역할 삭제는 부여 연결 RESTRICT·퍼미션 연결 cascade, 카탈로그 삭제는 연결 RESTRICT다. 감사·관리자 계정/역할 seed는 추가하지 않았다. (확인)
- migration 실행기가 번호 SQL 파일을 자동 수집하므로 `src/migrations.rs` 등록 변경은 필요 없다. SQL 자체 BEGIN/COMMIT, `IF NOT EXISTS`, 카탈로그 `ON CONFLICT DO NOTHING`으로 같은 스키마 재실행 시 기존 역할·구성·version을 보존한다. 다른 구조로 변형된 기존 테이블을 자동 보정하는 계약은 추가하지 않았다. (확인)
- [rbac/mod.rs](../../../../tlab-app/src/rbac/mod.rs), [entity.rs](../../../../tlab-app/src/rbac/entity.rs), [공통 Error](../../../../tlab/src/error.rs), [repository](../../../../tlab-app/src/rbac/repository/mod.rs)에 실제 타입·저장 계약만 구현했다. RoleId·RoleIds·PermissionCode는 검증되지 않은 생성/Deserialize 경로를 노출하지 않으며 SQL raw row도 읽을 때 검증한다. PermissionSet은 정렬·중복 제거·차집합·합집합과 128/4096 한도를 제공한다. (확인)
- 07 소유의 `main.rs`에는 `mod rbac`를 추가하지 않았다. [rbac_storage integration test](../../../../tlab-app/tests/rbac_storage.rs)가 `#[path = "../src/rbac/mod.rs"]`로 **실제 소스**를 컴파일하며, 현재 `app::db`와 같은 PostgreSQL Database/Context 타입 alias만 테스트 진입점에 둔다. 저장 동작을 복제하거나 가짜 resolver/route 파일을 만들지 않았다. 이 모듈은 서버 런타임에는 아직 연결되지 않는다. 후속 소비자를 위한 미사용 계약의 dead_code 허용은 이 테스트의 RBAC 모듈 한 곳에만 명시했다. (확인)
- 존재하지 않는 resolver/claims/authorization/policy/slice/route의 모듈 선언·재노출은 실제 파일 인계 시 `rbac/mod.rs` 소유자가 추가한다. 01 표의 선행 선언 계획 대신 실제 저장소만으로 컴파일 가능한 경로를 사용한 것이며 파일 소유권과 런타임 계약은 유지한다. 03·05·06은 루트 파일을 직접 바꾸지 않고 인계 요청한다. (확인·추론)

#### 후속 인계 API

| 위치 | 제공 API와 호출자 책임 |
| --- | --- |
| `rbac` 공개 재노출 | 검증된 entity·한도 상수·InputError/LookupError/ManagementError, `list_role_ids(ctx, account_id)`, `resolve_in_context(ctx, &RoleIds)` |
| `repository::role` | NewRole insert, ID find, ID 단건/정렬 중복 제거 다건 lock, ID cursor list(1~100개), RoleMetadata 조건부 update, 조건부 advance_version·delete. 다건 잠금은 actor+target 합집합이므로 입력을 RoleIds의 32개로 제한하지 않음 |
| 역할 version | 초기 1. 메타데이터 no-op은 version/updated_at 유지. 실제 metadata 변경·advance_version은 +1. 불일치/부재는 update의 None·delete의 false로 반환하며 usecase가 잠금 후 구분. permission 연결 쓰기와 advance_version은 **호출자의 같은 tx**에 조합하며 실패 시 전체 rollback |
| `repository::permission` | 정렬 catalog list·code find. 런타임 catalog 쓰기는 제공하지 않고 배포 SQL이 소유 |
| `repository::role_permission` | NewRolePermission insert(중복 false), 단건 delete·역할별 delete, 역할별 PermissionSet 조회, RoleIds 일괄 합집합. 한 SELECT에서 역할별 개수도 검사해 유효 합집합이 작아도 129개짜리 역할은 조회 실패 |
| `repository::account_role` | NewAccountRole insert(중복 None, 기존 granted_at 보존), 단건 delete(부재 false), 계정 역할 수·역할 부여 존재 조회. list_role_ids는 33개째까지 읽고 초과를 오류로 처리. 계정 자체의 존재/상태 판단은 별도 auth 조회 |
| 오류 | 예상 역할명 고유 충돌만 RoleNameConflict로 변환. 역할 삭제의 지정 FK RESTRICT만 RoleInUse로 변환(실제 DB의 SQLSTATE 23001 및 23503 대응). 다른 FK 실패는 Storage를 보존하며, UnknownPermission·대상 부재·위임·상태·계정 33번째 부여 거부 판단은 후속 usecase가 담당 |
| 트랜잭션·정책 경계 | repository는 주어진 Context만 사용하고 연결·tx를 열거나 commit하지 않음. 호출자가 계정→역할 정렬 잠금, 최종 관리 권한/범위, 교체 원자성, no-op 여부, version 실패 rollback을 소유. 자동 retry 없음 |

위 표는 02 구현된 계약이며 03 이후 기능이 구현되었다는 뜻이 아니다. (확인)

#### 검증 근거와 자체 점검

- `env TLAB_TEST_DATABASE_URL=postgres://tlab:tlab_dev@127.0.0.1:5432/tlab cargo test -p tlab-app --test rbac_storage --locked -- --include-ignored`: **6 passed, 0 failed, 0 ignored**. 타입/집합 경계 1개와 실제 PostgreSQL 시나리오 5개를 실행했다. 각 DB 시나리오는 UUID 고유 `tlab_rbac_test_*` 스키마와 그 스키마만 지정한 search_path를 사용하고, 실패를 포함해 종료 뒤 해당 스키마를 제거한다. 공유 업무 스키마·운영 데이터에는 migration을 적용하지 않았다. (확인)
- 실제 확인: 전체 migration 실행 및 재실행 후 catalog/역할/version/구성 보존, 인덱스 존재, FK·고유·형식 제약, 중복 부여/연결의 멱등성, 삭제 cascade/RESTRICT, 없는 역할·다중 역할 합집합, 33개 계정 역할·129개 역할 퍼미션·오염 코드·테이블 조회 장애의 오류, 메타데이터 no-op과 CAS, 부분 변경 뒤 FK 실패/rollback 복원, 실제 행 잠금 대기와 경합 version 변경 하나만 성공. (확인)
- 최초 실행에서 RESTRICT의 실제 SQLSTATE가 23001임을 확인해 역할 삭제 오류 변환과 기대값을 수정했다. 한 차례 재실행은 샌드박스의 PostgreSQL 연결 차단으로 실행 실패했으며, 허용된 로컬 DB 접근으로 재실행한 결과가 위 통과 근거다. 컴파일 중 발견한 테스트 Context 이중 mutable borrow도 수정했다. (확인)
- `cargo clippy -p tlab-app --all-targets --locked -- -D warnings`: 통과. 초기 single_match/collapsible_if 경고를 수정한 뒤 재실행했다. `cargo fmt --all --check`와 `git diff --check`도 통과했다. (확인)
- 자체 점검에서 정책 판단이 repository에 섞이지 않는지, raw DB 값의 검증 우회, 부분 결과 성공·조회 한도 절단, 삭제 FK 오류의 부여 실패 오분류, 소유권 밖 파일 변경을 확인했다. 부여된 역할 삭제 오류 분류는 delete 경로에만 두어 없는 역할 부여가 RoleInUse가 되지 않도록 했다. (확인)
- 통합 전 한계: production binary의 RBAC 모듈 연결·Container/HTTP·resolver trait·claims·최종 관리 정책·JWT 발급·CLI·실제 업무 인가는 미구현/미검증이다. 이 02 검증은 저장소와 같은 Context의 트랜잭션 계약을 확인하며 최종 인가를 검증한 것으로 보고하지 않는다. 직접 DB 조치가 타입/한도를 위반할 수 있어 조회는 이를 실패로 다룬다. (확인)

- 최종 코드의 PostgreSQL 검증을 다시 실행해 **6 passed, 0 failed, 0 ignored**를 확인했다. `cargo test --workspace --locked`는 기존 `app::tests::jwt_configuration_fails_before_connecting_to_database` 1개 실패로 종료했다(앱 binary: 28 passed, 1 failed, 42 ignored). 해당 테스트만 `cargo test -p tlab-app --bin tlab-app --locked app::tests::jwt_configuration_fails_before_connecting_to_database -- --exact`로 실행해도 같은 `app.rs:251` assertion 실패를 재현했다. 다른 DB 테스트의 ignored를 통과로 집계하지 않았다. (확인)
- 위 기존 실패는 checked-in `config/app.toml`의 `generate_if_missing=true`를 로드한 테스트가 첫 Container 생성에서 키를 만든 뒤, 이후 private_key가 없어야 한다고 검사하는 코드와 연결된다. RBAC는 아직 binary에 선언되지 않았고 `app.rs`·설정·main·의존성 파일은 이번에 수정하지 않았다. 02 저장소 검증은 통과했지만 **전체 workspace 테스트는 통과하지 않았다**. 이 기존 테스트 수정은 07 소유 파일 범위와도 겹치므로 별도 인계하며 여기서 변경하지 않는다. (확인: 재현·코드 / 추론: 원인)
- 02 완료: 승인된 스키마·타입·저장소 구현과 실제 격리 DB 검증·자체 점검 완료. 남은 사항은 기존 JWT 설정 테스트 실패와 03 이후 통합/정책/HTTP 검증이다. 커밋·배포·추가 에이전트 실행은 하지 않았다. (확인)

### 02 후속 수정: 오류 단순화

- 사용자가 상세한 RBAC 오류 분류 대신 `tlab::Error`와 DB 오류 사용을 요청했다. 이에 따라 InputError·LookupError·ManagementError와 `rbac/error.rs`를 제거하고, 타입 검증·repository 결과를 기존 `tlab::Result`로 통일했다. 위 02 최초 기록의 전용 오류·변환 설명은 이 수정으로 대체된다. (확인)
- 잘못된 입력은 InvalidInput, 저장 값의 형식/한도 위반은 IllegalState, SQL 실패는 원래 DbError(sqlx::Error)를 보존한다. 역할명 중복·부여 중 역할 삭제를 저장소에서 업무 오류로 재분류하지 않는다. 필요하면 후속 usecase가 SQLSTATE·constraint로 판단한다. (확인)
- 01의 후속 인터페이스도 같은 공통 오류 계약으로 갱신했다. 순수 정책은 bool, 조회가 필요한 인가는 tlab::Result<bool>로 허용/거부/조회 실패를 구분하도록 정리했다. 03 이후 구현은 진행하지 않았다. (확인·추론)

### 02 후속 수정: 근거 없는 MAX 제약 제거·타입 필요성 재검토

- 역할 32개·역할별 퍼미션 128개·합집합 4096개·JWT 3000 bytes는 요구나 측정 근거 없이 설계에서 추가한 제한이었다. 이를 제거하고 역할·퍼미션 조회는 전체 결과를 반환하며 SQL DISTINCT로 합집합을 만든다. 01의 대응 한도·발급·초기화 설명도 갱신했다. 기존 웹 쿠키 4000 bytes 전달 검사는 유지한다. (확인·추론)
- PermissionSet 생성·합집합은 이제 개수 제한 실패가 없으므로 from_codes/union이 직접 집합을 반환한다. 구조적 ID/코드 검증과 실제 DB 제약은 유지한다. (확인)
- 사용자는 RoleId·RoleIds·PermissionCode 등의 필요성 재검토를 요청했다. 실제 보장 내용은 양수·코드 형식·정렬/중복 제거이며 기존 auth는 ID를 i64로 직접 쓴다. 지금 범위에는 i64·Vec<i64>/&[i64]·String/&str·BTreeSet<String>으로 충분하고, 입력 경계에서 검증·정규화하는 안을 권장한다. **조사 결과이며 타입 제거는 아직 적용하지 않았다.** 역할/퍼미션/연결의 저장 entity 자체는 CRUD 입력·결과로 필요하다. (확인: 코드 / 추론: 권장안)

### 타입 유지에 대한 사용자 판단 반영

- 사용자가 우선 타입으로 의미를 명시하는 방법을 유지하는 쪽으로 의견을 줬다. RoleId·RoleIds·PermissionCode·PermissionSet을 유지하며, 위 기본 타입 전환 권장안은 채택하지 않는다. 타입의 책임은 양수/코드 형식 검증·정렬·중복 제거·집합 연산으로 한정한다. 임의 개수 상한은 복구하지 않는다. (확인)
- 오류 단순화 및 MAX 제거 이후 최종 검증: PostgreSQL 고유 스키마에서 6 passed, 0 failed, 0 ignored. 예전 상한을 넘는 역할 33개·역할 퍼미션 129개·집합 4097개가 정상 처리되는 검증으로 교체했고, FK/중복/version/rollback/잠금/migration 재실행 및 오염 데이터/DB 장애 검증은 유지했다. clippy(all-targets, -D warnings)·fmt check·diff check도 통과했다. 기존 workspace JWT 설정 테스트 실패는 앞선 기록과 동일한 미해결 사항이며 이번 국소 변경에서는 전체 workspace를 재실행하지 않았다. (확인)

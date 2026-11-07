-- PostgreSQL. 기준: docs/specs/auth/account.md
-- 계정은 프로필·상태를, 로그인 수단은 인증 식별자·이메일·우선순위를 소유한다.
-- 가입은 하나의 트랜잭션에서 다음 순서로 수행한다.
-- 1. 계정을 INSERT하고 RETURNING id로 계정 ID를 받는다.
-- 2. 해당 account_id로 identity를 INSERT하고 RETURNING id로 identity ID를 받는다.
-- 3. COMMIT한다. 가입 완료 시 하나 이상의 identity가 존재하는지는 앱에서 보장한다.
-- 계정별 ORDER BY priority ASC, id ASC LIMIT 1로 연락처로 사용할 identity를 선택한다.
-- priority는 작은 값이 우선이며 중복을 허용한다. 동률은 id가 작은 수단을 먼저 선택한다.
-- 지원 제공자·인증 정보 조합·문자열 정리와 유효성 검증도 앱에서 관리한다.
-- 현재 가입 흐름은 수단 하나만 생성한다. 여러 수단 연결·우선순위 변경 API는 후속 범위다.
--
-- 설계 가정(추론): 로컬 이메일은 대소문자 구분 없이 고유하며 닉네임 중복은 허용한다.
-- SNS 이메일은 계정 연결 키가 아니다. 같은 이메일이어도 자동 연결하지 않는다.
-- 로컬 이메일 조회에는 lower(email)을 사용하고 입력의 앞뒤 공백은 앱에서 제거한다.
-- 이메일 소유 확인 및 SNS 이메일 갱신 정책은 앱에서 별도 결정한다.
-- 계정 상태는 REGISTERED(가입), ACTIVE(활성), WITHDRAWN(탈퇴)로 구분한다.
-- 생성 기본값은 REGISTERED이며 활성 전환 조건과 허용 전이는 앱에서 결정한다.
-- 이메일 소유 확인 시각과 계정 활성 상태를 자동으로 연동하지 않는다.
--
-- 탈퇴는 상태 변경으로 표현하며 로그인·세션 차단은 앱의 책임이다.
-- 보관 기간·익명화·재가입 정책은 미정이다. 행을 보관하는 동안 인증 식별자의
-- 고유성도 유지되므로, 재가입을 허용하려면 별도 정리 정책이 필요하다.
-- 계정을 물리 삭제하면 소유한 로그인 수단도 삭제된다. 서비스 데이터는 다루지 않는다.
-- updated_at은 변경 SQL에서 앱이 갱신한다. DEFAULT는 최초 INSERT에만 적용된다.
--
-- 기존 실행기는 이력 없이 전체 파일을 재실행한다. 동일 스키마의 재실행만 지원하며
-- 기존 테이블의 구조를 바꾸는 작업은 후속 번호의 SQL로 작성한다.

BEGIN;

CREATE TABLE IF NOT EXISTS tlab_account (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    nickname TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'REGISTERED',
    withdrawn_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT tlab_account_status_check
        CHECK (status IN ('REGISTERED', 'ACTIVE', 'WITHDRAWN')),
    CONSTRAINT tlab_account_withdrawn_at_check
        CHECK ((status = 'WITHDRAWN') = (withdrawn_at IS NOT NULL))
);

CREATE TABLE IF NOT EXISTS tlab_account_identity (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id BIGINT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    provider TEXT NOT NULL,
    provider_subject TEXT,
    email TEXT NOT NULL,
    email_verified_at TIMESTAMPTZ,
    password_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT tlab_account_identity_account_fk
        FOREIGN KEY (account_id) REFERENCES tlab_account (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS tlab_account_identity_account_priority_idx
    ON tlab_account_identity (account_id, priority, id);

CREATE UNIQUE INDEX IF NOT EXISTS tlab_account_identity_local_email_key
    ON tlab_account_identity (lower(email))
    WHERE provider = 'LOCAL';

CREATE UNIQUE INDEX IF NOT EXISTS tlab_account_identity_provider_subject_key
    ON tlab_account_identity (provider, provider_subject)
    WHERE provider_subject IS NOT NULL;

COMMENT ON TABLE tlab_account IS '사용자 계정. 프로필·상태와 서비스 데이터의 소유 주체';
COMMENT ON COLUMN tlab_account.id IS '계정 내부 숫자 ID. 서비스 데이터가 참조할 사용자 식별자';
COMMENT ON COLUMN tlab_account.nickname IS '필수 닉네임. 중복 허용. 공백 정리와 유효성 검증은 앱 책임';
COMMENT ON COLUMN tlab_account.status IS '계정 상태: REGISTERED(가입, 기본값), ACTIVE(활성), WITHDRAWN(탈퇴). 활성 전환 조건·허용 전이·로그인 및 세션 접근은 앱에서 판단';
COMMENT ON COLUMN tlab_account.withdrawn_at IS '탈퇴 시각. WITHDRAWN일 때만 필수이며 정보 삭제·보관 기간을 의미하지 않음';
COMMENT ON COLUMN tlab_account.created_at IS '계정 생성 시각';
COMMENT ON COLUMN tlab_account.updated_at IS '계정 최종 변경 시각. 변경 SQL에서 앱이 갱신';

COMMENT ON TABLE tlab_account_identity IS '계정에 속한 로컬 또는 SNS 로그인 수단. 이메일이 같아도 다른 수단·계정과 자동 연결하지 않음';
COMMENT ON COLUMN tlab_account_identity.id IS '로그인 수단 내부 숫자 ID. INSERT 시 자동 생성';
COMMENT ON COLUMN tlab_account_identity.account_id IS '로그인 수단을 소유한 계정. 계정 물리 삭제 시 함께 삭제';
COMMENT ON COLUMN tlab_account_identity.priority IS '계정 내 로그인 수단 우선순위. 작은 값 우선, 중복 허용, 동률은 id 오름차순. 첫 수단의 이메일을 계정 연락처로 사용';
COMMENT ON COLUMN tlab_account_identity.provider IS '인증 제공자 코드. 예: LOCAL, GOOGLE, KAKAO. 지원 목록과 코드 표준화는 앱에서 관리';
COMMENT ON COLUMN tlab_account_identity.provider_subject IS '제공자가 발급한 안정적인 사용자 식별자. 예: GOOGLE sub, KAKAO 사용자 ID. 필수 여부와 형식은 앱에서 검증';
COMMENT ON COLUMN tlab_account_identity.email IS '필수 이메일. LOCAL은 대소문자 무시 고유 로그인 ID, SNS는 연락처 정보';
COMMENT ON COLUMN tlab_account_identity.email_verified_at IS '앱이 신뢰하는 이메일 소유 확인 시각. NULL은 소유 확인 미완료 또는 미확인';
COMMENT ON COLUMN tlab_account_identity.password_hash IS 'PHC 형식 비밀번호 해시. 원문 저장 금지. 제공자별 필요 여부와 해시 생성·검증은 앱 책임';
COMMENT ON COLUMN tlab_account_identity.created_at IS '로그인 수단 생성 시각';
COMMENT ON COLUMN tlab_account_identity.updated_at IS '로그인 수단 최종 변경 시각. 변경 SQL에서 앱이 갱신';
COMMENT ON INDEX tlab_account_identity_account_priority_idx IS '계정별 로그인 수단을 priority, id 순으로 조회하고 우선 연락처 선택';
COMMENT ON INDEX tlab_account_identity_local_email_key IS 'LOCAL 이메일의 대소문자 무시 중복 방지. SNS 이메일에는 고유성 제약 없음';
COMMENT ON INDEX tlab_account_identity_provider_subject_key IS '같은 SNS 사용자 식별자가 여러 로그인 수단·계정에 연결되는 것을 방지';

COMMIT;

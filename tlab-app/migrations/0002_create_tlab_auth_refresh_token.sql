-- PostgreSQL. 기준: docs/specs/auth/jwt.md, 0001_create_tlab_account.sql
-- 현재 정책: 로그인마다 Refresh 하나를 발급하고 최대 7일 동안 반복 사용한다.
-- 갱신은 Access만 발급한다. Refresh 교체·사용 이력·재사용 감지는 현재 범위에서 제외한다.
-- 별도 세션 테이블 없이 Refresh 행이 계정·로그인 ID·만료·폐기 상태를 소유한다.
-- session_id는 로그인마다 생성하는 UUID v4이며 Access JWT의 sid에 문자열로 담는다.
-- Access의 sub·sid는 검증한 Refresh 행의 account_id·session_id에서 구성한다.
-- 일반 API는 JWT를 검증하며 DB를 매번 조회하지 않는다. Refresh를 폐기한 뒤에도
-- 기존 Access는 만료까지 최대 10분 허용한다. sid 자체는 인증 비밀값이 아니다.
--
-- 앱 처리 계약: 로그인·로그아웃과 계정 상태 변경·전체 폐기 조합은 호출자가 트랜잭션을 소유한다.
-- 로그인과 계정 상태 변경·전체 폐기는 계정 행 잠금 아래 직렬화한다.
-- 갱신은 일반 연결에서 행 잠금 없이 Refresh와 계정을 각각 SELECT하고 현재 시각으로 만료를 판정한다.
-- 갱신에는 명시적 트랜잭션·COMMIT이 없으며 두 조회의 동일 시점 스냅샷을 보장하지 않는다.
-- 일반 로그아웃은 선행 계정 잠금 없이 미폐기 행을 조건부 UPDATE한다.
-- 갱신이 동시 폐기와 겹치면 이전 상태로 Access를 발급할 수 있다.
--
-- 로그인: 계정 잠금과 로그인 허용 상태 확인 후 난수 Refresh의 SHA-256 해시를 INSERT.
--   최초 로그인 시각과 로그인 유지 한도(168시간 후)를 지정한다.
--   현재 expires_at = session_expires_at으로 지정하고 COMMIT 후 토큰을 전달한다.
-- 갱신: 계정 허용 상태, revoked_at IS NULL, now < expires_at 및
--   now < session_expires_at을 확인한다. 최신 사용자·권한으로 Access만 발급한다.
--   Refresh·만료 시각·session_id는 변경하지 않는다. 동시 갱신·재시도도 허용한다.
--   재시도해도 Refresh·세션 만료는 최초 로그인부터 168시간으로 고정되며 연장하지 않는다.
-- 로그아웃: 검증한 Refresh의 session_id에 속한 미폐기 행을 LOGOUT으로 폐기한다.
-- 정지·탈퇴: 상태 변경과 계정의 모든 미폐기 Refresh 폐기를 함께 수행한다.
--   기존 계정 모델에 정지 상태가 없으므로 상태 모델과 로그인 허용 정책 연계는 후속 구현이다.
-- 폐기 시 기존 사유·시각은 덮어쓰지 않는다. 만료는 시각 비교로 판단하며 별도 상태값은 없다.
-- 만료 후 보관 기간은 미정이며 만료·폐기된 행의 정리는 별도 정책으로 결정한다.
--
-- 확장 경계: expires_at은 토큰 수명, session_expires_at은 로그인 유지 한도이다.
-- 추후 교체 발급 시 session_id·account_id·session_created_at·session_expires_at을 계승하고
-- 새 expires_at = min(발급 시각 + 토큰 수명, session_expires_at)으로 정할 수 있다.
-- 기존 행 폐기 후 새 행 삽입은 같은 계정 잠금 아래 하나의 트랜잭션으로 수행해야 한다.
-- 수명 단축만으로 교체 발급이 구현되지는 않는다. 교체 사유·이력 보존·재사용 감지 정책과
-- 앱 처리는 도입 시 별도 추가한다. 로그인 공통 정보의 불변성과 행 간 일관성은 앱 책임이다.
--
-- 실행기는 이력 없이 재실행하므로 동일 스키마의 재실행만 허용한다.
-- 이 파일은 미적용 설계안 기준이다. 이전 세션 분리 버전을 이미 적용한 DB의 구조 변경은
-- CREATE TABLE IF NOT EXISTS로 처리되지 않으며 별도 후속 마이그레이션이 필요하다.

BEGIN;

CREATE TABLE IF NOT EXISTS tlab_auth_refresh_token (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    session_id UUID NOT NULL DEFAULT gen_random_uuid(),
    account_id BIGINT NOT NULL,
    token_hash BYTEA NOT NULL,
    session_created_at TIMESTAMPTZ NOT NULL,
    session_expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    revoke_reason TEXT,

    CONSTRAINT tlab_auth_refresh_token_account_fk
        FOREIGN KEY (account_id) REFERENCES tlab_account (id) ON DELETE CASCADE,
    CONSTRAINT tlab_auth_refresh_token_hash_key UNIQUE (token_hash),
    CONSTRAINT tlab_auth_refresh_token_hash_length_check
        CHECK (octet_length(token_hash) = 32),
    CONSTRAINT tlab_auth_refresh_token_session_uuid_v4_check
        CHECK ((get_byte(uuid_send(session_id), 6) & 240) = 64
           AND (get_byte(uuid_send(session_id), 8) & 192) = 128),
    CONSTRAINT tlab_auth_refresh_token_session_expiry_check
        CHECK (session_expires_at = session_created_at + INTERVAL '168 hours'),
    CONSTRAINT tlab_auth_refresh_token_created_at_check
        CHECK (created_at >= session_created_at),
    CONSTRAINT tlab_auth_refresh_token_expiry_check
        CHECK (expires_at > created_at AND expires_at <= session_expires_at),
    CONSTRAINT tlab_auth_refresh_token_revocation_check
        CHECK ((revoked_at IS NULL) = (revoke_reason IS NULL)),
    CONSTRAINT tlab_auth_refresh_token_revoked_at_check
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    CONSTRAINT tlab_auth_refresh_token_revoke_reason_check
        CHECK (revoke_reason IN ('LOGOUT', 'ACCOUNT_SUSPENDED', 'ACCOUNT_WITHDRAWN'))
);

CREATE INDEX IF NOT EXISTS tlab_auth_refresh_token_account_idx
    ON tlab_auth_refresh_token (account_id);

CREATE INDEX IF NOT EXISTS tlab_auth_refresh_token_session_idx
    ON tlab_auth_refresh_token (session_id);

CREATE INDEX IF NOT EXISTS tlab_auth_refresh_token_expires_at_idx
    ON tlab_auth_refresh_token (expires_at);

CREATE UNIQUE INDEX IF NOT EXISTS tlab_auth_refresh_token_unrevoked_key
    ON tlab_auth_refresh_token (session_id) WHERE revoked_at IS NULL;

COMMENT ON TABLE tlab_auth_refresh_token IS '로그인별 Refresh 해시·만료·폐기 정보. 현재 동일 Refresh를 최대 7일 반복 사용하고 Access만 갱신';
COMMENT ON COLUMN tlab_auth_refresh_token.session_id IS '로그인 UUID v4. Access JWT의 sid. 같은 로그인에서 유지하며 인증 비밀값으로 사용하지 않음';
COMMENT ON COLUMN tlab_auth_refresh_token.account_id IS '로그인한 계정. 로그인과 계정 상태 변경·전체 폐기는 계정 잠금 아래 처리';
COMMENT ON COLUMN tlab_auth_refresh_token.token_hash IS '고엔트로피 난수 Refresh 원문의 SHA-256 해시 32바이트. 원문 저장 금지';
COMMENT ON COLUMN tlab_auth_refresh_token.session_created_at IS '최초 로그인 시각. Access 갱신 시 변경하지 않음';
COMMENT ON COLUMN tlab_auth_refresh_token.session_expires_at IS '최초 로그인부터 168시간 후인 로그인 유지 한도. 갱신 시 연장하지 않음';
COMMENT ON COLUMN tlab_auth_refresh_token.created_at IS '해당 Refresh 발급 시각';
COMMENT ON COLUMN tlab_auth_refresh_token.expires_at IS '해당 Refresh 만료 시각. 현재 로그인 유지 한도와 같게 발급하며 추후 더 짧은 수명 허용';
COMMENT ON COLUMN tlab_auth_refresh_token.revoked_at IS '만료 전 강제 폐기 시각. NULL이어도 만료된 토큰은 갱신 불가';
COMMENT ON COLUMN tlab_auth_refresh_token.revoke_reason IS '폐기 사유: LOGOUT, ACCOUNT_SUSPENDED, ACCOUNT_WITHDRAWN. 첫 폐기 결과 보존';
COMMENT ON INDEX tlab_auth_refresh_token_account_idx IS '계정별 전체 Refresh 폐기 및 계정 삭제의 참조 조회';
COMMENT ON INDEX tlab_auth_refresh_token_session_idx IS '로그인별 Refresh 조회와 전체 폐기';
COMMENT ON INDEX tlab_auth_refresh_token_expires_at_idx IS '만료된 Refresh 조회 및 정리';
COMMENT ON INDEX tlab_auth_refresh_token_unrevoked_key IS '로그인마다 미폐기 Refresh 최대 하나. 만료 여부는 별도 시각 비교';

COMMIT;

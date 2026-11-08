-- Repeatable on the same schema; later structural changes need a new number.
-- Authorization, delegation, assignment limits and transaction ownership belong
-- to usecases. No role or administrator account is seeded here.
BEGIN;

CREATE TABLE IF NOT EXISTS tlab_rbac_permission (
    code TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    CONSTRAINT tlab_rbac_permission_code_check CHECK (
        octet_length(code) BETWEEN 1 AND 64
        AND code COLLATE "C" ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$'
    )
);

CREATE TABLE IF NOT EXISTS tlab_rbac_role (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY CHECK (id > 0),
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT tlab_rbac_role_name_key UNIQUE (name),
    CONSTRAINT tlab_rbac_role_name_check CHECK (length(btrim(name)) > 0)
);

CREATE TABLE IF NOT EXISTS tlab_rbac_role_permission (
    role_id BIGINT NOT NULL,
    permission_code TEXT NOT NULL,
    PRIMARY KEY (role_id, permission_code),
    CONSTRAINT tlab_rbac_role_permission_role_fk FOREIGN KEY (role_id)
        REFERENCES tlab_rbac_role (id) ON DELETE CASCADE,
    CONSTRAINT tlab_rbac_role_permission_permission_fk FOREIGN KEY (permission_code)
        REFERENCES tlab_rbac_permission (code) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS tlab_rbac_role_permission_permission_idx
    ON tlab_rbac_role_permission (permission_code, role_id);

CREATE TABLE IF NOT EXISTS tlab_rbac_account_role (
    account_id BIGINT NOT NULL,
    role_id BIGINT NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (account_id, role_id),
    CONSTRAINT tlab_rbac_account_role_account_fk FOREIGN KEY (account_id)
        REFERENCES tlab_account (id) ON DELETE CASCADE,
    CONSTRAINT tlab_rbac_account_role_role_fk FOREIGN KEY (role_id)
        REFERENCES tlab_rbac_role (id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS tlab_rbac_account_role_role_idx
    ON tlab_rbac_account_role (role_id, account_id);

INSERT INTO tlab_rbac_permission (code, description) VALUES
    ('account.create', 'Create local accounts'),
    ('rbac.read', 'Read the permission catalog, roles and other accounts access'),
    ('rbac.role.manage', 'Create, edit and delete roles and their permissions'),
    ('rbac.assignment.manage', 'Grant and revoke account roles')
ON CONFLICT (code) DO NOTHING;

COMMIT;

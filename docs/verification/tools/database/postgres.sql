BEGIN;
CREATE TEMPORARY TABLE tlab_verification (value integer) ON COMMIT DROP;
INSERT INTO tlab_verification VALUES (42);
DO $$
BEGIN
    IF (SELECT value FROM tlab_verification) IS DISTINCT FROM 42 THEN
        RAISE EXCEPTION 'PostgreSQL read/write check failed';
    END IF;
END
$$;
ROLLBACK;
SELECT 'PostgreSQL read/write OK' AS result;

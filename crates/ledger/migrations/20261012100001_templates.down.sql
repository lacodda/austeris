-- The previous schema has no word for a template. Rolling back over one would
-- drop a button the person made, so it is refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM templates) THEN
        RAISE EXCEPTION 'templates are recorded, and the previous schema would drop them; delete them before rolling back';
    END IF;
END;
$$;

DROP INDEX entries_typed_recently_idx;
DROP TABLE template_tags;
DROP TABLE templates;
ALTER TABLE accounts DROP CONSTRAINT accounts_owner_id_key;

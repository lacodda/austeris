-- The previous schema has no word for a tax number. Rolling back over one would
-- lose what every later receipt from that shop is matched by, so it is refused
-- rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM counterparties WHERE tax_id IS NOT NULL) THEN
        RAISE EXCEPTION 'counterparties carry tax numbers, and the previous schema would drop them; clear them before rolling back';
    END IF;
END;
$$;

DROP INDEX counterparties_owner_tax_id_key;
ALTER TABLE counterparties DROP COLUMN tax_id;

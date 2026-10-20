-- Before this schema there was nowhere a document waited. Rolling back over one
-- still waiting would drop a receipt nobody has booked yet, so it is refused
-- rather than done quietly; what was booked is in the ledger already.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM documents WHERE state = 'waiting') THEN
        RAISE EXCEPTION 'documents are waiting to be booked, and rolling back would drop them; book or dismiss them first';
    END IF;
END;
$$;

DROP TABLE documents;
DROP TYPE document_origin;
DROP TYPE document_state;

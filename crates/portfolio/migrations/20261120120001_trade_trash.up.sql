-- A deleted trade can come back, for as long as the trash keeps it.
--
-- Deleting a trade has always left its history behind: the revision of its
-- deletion holds the trade as it was. That is what the trash is made of, as
-- it is for an entry in the ledger (ADR 0013): nothing is marked deleted and
-- left where a position reads it, so no sum can forget to leave it out.
-- Restoring writes the trade back under its own id, which is the key its
-- money is posted under in the books - the ledger answers the same key with
-- the same entry - and the history says it came back. After the days the
-- trash keeps things, the history of a trade still deleted is erased with it.

ALTER TYPE revision_action ADD VALUE IF NOT EXISTS 'restored';

-- A trade as it was, now with the key its client named it by and when it was
-- first recorded: what a restored trade has to be written back with to be
-- the same trade - an import run again finds it by that key.
CREATE OR REPLACE FUNCTION trade_snapshot(subject uuid) RETURNS jsonb AS $$
    SELECT jsonb_build_object(
        'kind', t.kind,
        'account_id', t.account_id,
        'to_account_id', t.to_account_id,
        'instrument_id', t.instrument_id,
        'quantity', trim_scale(t.quantity)::text,
        'price', trim_scale(t.price)::text,
        'currency', t.currency,
        'fee', trim_scale(t.fee)::text,
        'occurred_at', t.occurred_at,
        'occurred_on', t.occurred_on,
        'note', t.note,
        'entry_id', t.entry_id,
        'idempotency_key', t.idempotency_key,
        'created_at', t.created_at
    )
      FROM trades t
     WHERE t.id = subject
$$ LANGUAGE sql STABLE;

-- A trade that came back was not anything before it came back, as one just
-- recorded was not. Compared as text: the new action cannot be named as one
-- in the transaction that adds it.
ALTER TABLE trade_revisions DROP CONSTRAINT trade_revisions_before_when_there_was_one;
ALTER TABLE trade_revisions ADD CONSTRAINT trade_revisions_before_when_there_was_one
    CHECK ((before IS NULL) = (action::text IN ('created', 'restored')));

CREATE OR REPLACE FUNCTION trade_touched(subject uuid, act revision_action, owner uuid) RETURNS void AS $$
DECLARE
    said text := current_setting('austeris.actor', true);
    present trade_revisions%ROWTYPE;
    who revision_actor;
    person uuid;
    module text;
BEGIN
    SELECT * INTO present FROM trade_revisions WHERE trade_id = subject AND txid = pg_current_xact_id();
    IF FOUND THEN
        IF act = 'deleted' AND present.action::text IN ('created', 'restored') THEN
            -- Made and unmade in one transaction: nothing anyone could see.
            DELETE FROM trade_revisions WHERE id = present.id;
        ELSIF act = 'deleted' THEN
            UPDATE trade_revisions SET action = 'deleted' WHERE id = present.id;
        END IF;
        RETURN;
    END IF;

    IF said IS NULL OR said = '' THEN
        IF current_setting('application_name', true) = 'austeris' THEN
            RAISE EXCEPTION 'austeris changed trade % without saying on whose behalf', subject;
        END IF;
        who := 'outside';
    ELSIF said LIKE 'person:%' THEN
        who := 'person';
        person := substr(said, 8)::uuid;
    ELSIF said LIKE 'module:%' THEN
        who := 'module';
        module := NULLIF(substr(said, 8), '');
    ELSE
        RAISE EXCEPTION 'austeris.actor is `%`, which names nobody', said;
    END IF;

    INSERT INTO trade_revisions (owner_id, trade_id, action, actor, actor_person, actor_module, before)
    VALUES (owner, subject, act, who, person, module,
            CASE WHEN act::text IN ('created', 'restored') THEN NULL ELSE trade_snapshot(subject) END);
END;
$$ LANGUAGE plpgsql;

-- A trade written under an id that has a history is one coming back, not one
-- recorded: its history goes on rather than starting again.
CREATE OR REPLACE FUNCTION trades_remembered() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM trade_touched(
            NEW.id,
            (CASE WHEN EXISTS (SELECT 1 FROM trade_revisions WHERE trade_id = NEW.id) THEN 'restored' ELSE 'created' END)::revision_action,
            NEW.owner_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        PERFORM trade_touched(OLD.id, 'changed', OLD.owner_id);
        RETURN NEW;
    END IF;
    PERFORM trade_touched(OLD.id, 'deleted', OLD.owner_id);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

-- The trash reads a person's deletions newest first, and the hourly sweep
-- the old ones; both start from the deletions.
CREATE INDEX trade_revisions_deleted_idx ON trade_revisions (owner_id, at) WHERE action = 'deleted';

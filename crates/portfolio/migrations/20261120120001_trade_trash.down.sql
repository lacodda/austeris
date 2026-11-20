-- The previous schema has no word for a trade that came back. Rolling back over
-- one would leave a history whose second life reads as a second recording, so
-- it is refused rather than done quietly. A value cannot leave an enum: the
-- word stays in the type, unused, and the rest is put back as it was.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM trade_revisions WHERE action::text = 'restored') THEN
        RAISE EXCEPTION 'trades came back from the trash, which the previous schema cannot say; erase their history before rolling back';
    END IF;
END;
$$;

DROP INDEX trade_revisions_deleted_idx;

CREATE OR REPLACE FUNCTION trades_remembered() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM trade_touched(NEW.id, 'created', NEW.owner_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        PERFORM trade_touched(OLD.id, 'changed', OLD.owner_id);
        RETURN NEW;
    END IF;
    PERFORM trade_touched(OLD.id, 'deleted', OLD.owner_id);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

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
        IF act = 'deleted' AND present.action = 'created' THEN
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
            CASE WHEN act = 'created' THEN NULL ELSE trade_snapshot(subject) END);
END;
$$ LANGUAGE plpgsql;

ALTER TABLE trade_revisions DROP CONSTRAINT trade_revisions_before_when_there_was_one;
ALTER TABLE trade_revisions ADD CONSTRAINT trade_revisions_before_when_there_was_one
    CHECK ((before IS NULL) = (action = 'created'));

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
        'entry_id', t.entry_id
    )
      FROM trades t
     WHERE t.id = subject
$$ LANGUAGE sql STABLE;

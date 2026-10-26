-- Money that became something a module holds, entries a module keeps, and
-- currencies that are not a state's.
--
-- A trade turns money into a coin or a share, and the ledger does not know
-- what either is (ADR 0001). Until now the only places money could go were an
-- account, a category or a conversion - and buying a coin is none of them: not
-- an account (the coin is no currency of the person's), not a category (it was
-- not spent, and a report of spending must not count it), not a conversion
-- (nothing arrives in another currency the ledger keeps). So a fourth side,
-- `holding`: money that left for something a module holds, or came back from
-- one. It names nothing, like a conversion - which coin is the module's to
-- know - and every currency still sums to zero on its own.
--
-- What a trade moved is decided by the trade, and the trade lives in its
-- module. An entry a person could delete or move to another day would tell a
-- different story from the trade it came from, so a module can keep the entry
-- it posts: it alone changes or takes it back, by the key it posted it under,
-- and a person is told where to change it instead (ADR 0015).
--
-- A stablecoin is money on an exchange - the cash a trade is paid from - so it
-- is a currency an account can be in. Its code is the one exchanges use, and
-- `USDT` has four letters.

ALTER TYPE line_side ADD VALUE 'holding';

-- Kept by the module that posted it: `source`, under `idempotency_key`, which
-- is how that module finds it again. Set when the entry is posted and never
-- after - an entry does not change hands.
ALTER TABLE entries ADD COLUMN kept boolean NOT NULL DEFAULT false;
ALTER TABLE entries ADD CONSTRAINT entries_kept_by_the_module_that_posted_it
    CHECK (NOT kept OR (source IS NOT NULL AND idempotency_key IS NOT NULL));

CREATE FUNCTION entry_kept_is_fixed() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'entry % is kept by the module that posted it or by nobody from the start, and stays so', NEW.id
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entries_kept_fixed
    BEFORE UPDATE OF kept ON entries
    FOR EACH ROW WHEN (NEW.kept IS DISTINCT FROM OLD.kept)
    EXECUTE FUNCTION entry_kept_is_fixed();

-- What a kept entry says about the money - its day, its description, its
-- lines - is changed by the module keeping it and by nobody else austeris
-- writes for. A person is refused here as well as on the way in: the rule
-- holds for every path that writes, the ones not written yet included. Who is
-- read from `austeris.actor`, as the history reads it; a write from outside
-- austeris - an operator's repair in psql - is let through, and the history
-- says it was one.
CREATE FUNCTION kept_entry_changed_by_its_module(subject uuid) RETURNS void AS $$
DECLARE
    said text := current_setting('austeris.actor', true);
    keeper text;
BEGIN
    SELECT source INTO keeper FROM entries WHERE id = subject AND kept;
    IF FOUND AND said IS NOT NULL AND said <> '' AND said <> 'module:' || keeper THEN
        RAISE EXCEPTION 'entry % is kept by %, and is changed there', subject, keeper
            USING ERRCODE = 'AUK01', DETAIL = keeper;
    END IF;
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION kept_entry_guarded() RETURNS trigger AS $$
BEGIN
    PERFORM kept_entry_changed_by_its_module(OLD.id);
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entries_kept_by_their_module
    BEFORE UPDATE OF occurred_on, description OR DELETE ON entries
    FOR EACH ROW WHEN (OLD.kept)
    EXECUTE FUNCTION kept_entry_guarded();

CREATE FUNCTION kept_entry_line_guarded() RETURNS trigger AS $$
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') THEN
        PERFORM kept_entry_changed_by_its_module(OLD.entry_id);
    END IF;
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        PERFORM kept_entry_changed_by_its_module(NEW.entry_id);
        RETURN NEW;
    END IF;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entry_lines_kept_by_their_module
    BEFORE INSERT OR UPDATE OR DELETE ON entry_lines
    FOR EACH ROW EXECUTE FUNCTION kept_entry_line_guarded();

-- Money into or out of holdings is the business of whoever holds them, so a
-- holding line belongs to an entry a module keeps. A person cannot type one -
-- they would be saying money became a coin no trade knows about - and a module
-- posting one without keeping the entry would leave it to be deleted from
-- under the trade. Checked at commit, when the entry has its lines.
CREATE FUNCTION holding_kept_by_a_module() RETURNS trigger AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM entries WHERE id = NEW.entry_id AND kept) THEN
        RAISE EXCEPTION 'entry % moves money into or out of holdings, which only an entry a module keeps can do', NEW.entry_id
            USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- Compared as text: the new value cannot be named as itself in the
-- transaction that adds it.
CREATE CONSTRAINT TRIGGER entry_lines_holding_kept
    AFTER INSERT OR UPDATE ON entry_lines
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (NEW.side::text = 'holding')
    EXECUTE FUNCTION holding_kept_by_a_module();

-- The entry as it was, now saying whether a module keeps it: the trash leaves
-- out what a module took back, and only the module brings it back.
CREATE OR REPLACE FUNCTION entry_snapshot(subject uuid) RETURNS jsonb AS $$
    SELECT jsonb_build_object(
        'occurred_on', e.occurred_on,
        'status', e.status,
        'cleared_on', e.cleared_on,
        'description', e.description,
        'source', e.source,
        'idempotency_key', e.idempotency_key,
        'kept', e.kept,
        'created_at', e.created_at,
        'counterparty', CASE WHEN cp.id IS NULL THEN NULL ELSE jsonb_build_object('id', cp.id, 'name', cp.name) END,
        'place', CASE WHEN p.id IS NULL THEN NULL ELSE jsonb_build_object('id', p.id, 'country', p.country::text, 'city', p.city) END,
        'tags', COALESCE((
            SELECT jsonb_agg(jsonb_build_object('id', t.id, 'name', t.name) ORDER BY lower(t.name), t.id)
              FROM entry_tags et JOIN tags t ON t.id = et.tag_id
             WHERE et.entry_id = e.id), '[]'::jsonb),
        -- Amounts as text, trimmed of the zeros NUMERIC(38, 18) pads them
        -- with: a JSON number is a float to most readers (ADR 0004).
        'lines', COALESCE((
            SELECT jsonb_agg(jsonb_build_object(
                       'id', l.id, 'side', l.side, 'account_id', l.account_id, 'category_id', l.category_id,
                       'amount', trim_scale(l.amount)::text, 'currency', l.currency, 'note', l.note)
                   ORDER BY l.amount, l.id)
              FROM entry_lines l
             WHERE l.entry_id = e.id), '[]'::jsonb),
        'attachments', COALESCE((
            SELECT jsonb_agg(jsonb_build_object(
                       'id', a.id, 'name', a.name, 'media_type', a.media_type, 'size', a.size, 'created_at', a.created_at)
                   ORDER BY a.created_at, a.id)
              FROM attachments a
             WHERE a.entry_id = e.id AND a.removed_at IS NULL), '[]'::jsonb)
    )
      FROM entries e
      LEFT JOIN counterparties cp ON cp.id = e.counterparty_id
      LEFT JOIN places p ON p.id = e.place_id
     WHERE e.id = subject
$$ LANGUAGE sql STABLE;

-- A currency's code: ISO 4217's three letters, or an exchange's ticker for
-- money that is not a state's - USDT, USDC, FDUSD.
ALTER TABLE accounts ADD CONSTRAINT accounts_currency_is_a_code CHECK (currency ~ '^[A-Z]{3,5}$');
ALTER TABLE templates DROP CONSTRAINT templates_currency_check;
ALTER TABLE templates ADD CONSTRAINT templates_currency_check CHECK (currency ~ '^[A-Z]{3,5}$');

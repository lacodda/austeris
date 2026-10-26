-- The previous schema has no word for money in holdings, for an entry a module
-- keeps, or for a currency code longer than three letters. Rolling back over
-- any of them would leave a trade without its money, or a button that cannot
-- be pressed, so it is refused rather than done quietly.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM entries WHERE kept)
       OR EXISTS (SELECT 1 FROM entry_lines WHERE side::text = 'holding')
       OR EXISTS (SELECT 1 FROM entry_revisions
                   WHERE before->>'kept' = 'true' OR jsonb_path_exists(before, '$.lines[*] ? (@.side == "holding")')) THEN
        RAISE EXCEPTION 'entries are kept by a module or move money into holdings, which the previous schema cannot express; switch the module off and erase them before rolling back';
    END IF;
    IF EXISTS (SELECT 1 FROM templates WHERE currency !~ '^[A-Z]{3}$') THEN
        RAISE EXCEPTION 'buttons record in a currency whose code is not three letters, which the previous schema refuses; delete them before rolling back';
    END IF;
END;
$$;

ALTER TABLE templates DROP CONSTRAINT templates_currency_check;
ALTER TABLE templates ADD CONSTRAINT templates_currency_check CHECK (currency ~ '^[A-Z]{3}$');
ALTER TABLE accounts DROP CONSTRAINT accounts_currency_is_a_code;

CREATE OR REPLACE FUNCTION entry_snapshot(subject uuid) RETURNS jsonb AS $$
    SELECT jsonb_build_object(
        'occurred_on', e.occurred_on,
        'status', e.status,
        'cleared_on', e.cleared_on,
        'description', e.description,
        'source', e.source,
        'idempotency_key', e.idempotency_key,
        'created_at', e.created_at,
        'counterparty', CASE WHEN cp.id IS NULL THEN NULL ELSE jsonb_build_object('id', cp.id, 'name', cp.name) END,
        'place', CASE WHEN p.id IS NULL THEN NULL ELSE jsonb_build_object('id', p.id, 'country', p.country::text, 'city', p.city) END,
        'tags', COALESCE((
            SELECT jsonb_agg(jsonb_build_object('id', t.id, 'name', t.name) ORDER BY lower(t.name), t.id)
              FROM entry_tags et JOIN tags t ON t.id = et.tag_id
             WHERE et.entry_id = e.id), '[]'::jsonb),
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

DROP TRIGGER entry_lines_kept_by_their_module ON entry_lines;
DROP FUNCTION kept_entry_line_guarded();
DROP TRIGGER entries_kept_by_their_module ON entries;
DROP FUNCTION kept_entry_guarded();
DROP FUNCTION kept_entry_changed_by_its_module(uuid);
DROP TRIGGER entry_lines_holding_kept ON entry_lines;
DROP FUNCTION holding_kept_by_a_module();
DROP TRIGGER entries_kept_fixed ON entries;
DROP FUNCTION entry_kept_is_fixed();
ALTER TABLE entries DROP CONSTRAINT entries_kept_by_the_module_that_posted_it;
ALTER TABLE entries DROP COLUMN kept;

-- An enum keeps a value once added; the type is made again without it. No
-- line holds it - that was refused above.
-- The rule tying a side to its references names the type in its literals, so
-- it is written again for the new one.
ALTER TABLE entry_lines DROP CONSTRAINT entry_lines_side_matches;
ALTER TYPE line_side RENAME TO line_side_with_holdings;
CREATE TYPE line_side AS ENUM ('account', 'category', 'conversion');
ALTER TABLE entry_lines ALTER COLUMN side TYPE line_side USING side::text::line_side;
DROP TYPE line_side_with_holdings;
ALTER TABLE entry_lines ADD CONSTRAINT entry_lines_side_matches CHECK (
    (side = 'account') = (account_id IS NOT NULL)
    AND (side = 'category') = (category_id IS NOT NULL)
);

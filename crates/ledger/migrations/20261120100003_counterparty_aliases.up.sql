-- What a counterparty merged into another was typed as.
--
-- A typed line names a counterparty by the first `@name`, and a typo names a
-- second one: "supersies" beside "Superseis". Merging the first into the
-- second moves its entries and its buttons, and the typo is gone - until it is
-- typed again, and makes a third. So the way it was typed stays, as a way of
-- typing the one it was merged into: `@supersies` finds Superseis from then
-- on, and a receipt printed under that name does too.
--
-- A name the person gives a counterparty is that counterparty's, and an alias
-- typed the same way gives way to it: a shop called "Supersies" opening next
-- door is a new shop, said on purpose.
CREATE TABLE counterparty_aliases (
    owner_id        uuid        NOT NULL,
    -- Typed as a counterparty's key is: `typed_key` of the name it stood for.
    key             text        NOT NULL CHECK (key <> ''),
    counterparty_id uuid        NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, key),
    CONSTRAINT counterparty_aliases_counterparty_fkey FOREIGN KEY (owner_id, counterparty_id)
        REFERENCES counterparties (owner_id, id) ON DELETE CASCADE
);

CREATE INDEX counterparty_aliases_counterparty_idx ON counterparty_aliases (counterparty_id);

CREATE FUNCTION counterparty_alias_gives_way() RETURNS trigger AS $$
BEGIN
    DELETE FROM counterparty_aliases WHERE owner_id = NEW.owner_id AND key = NEW.key;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- After the row is written, when its key - derived from the name - is known.
CREATE TRIGGER counterparties_take_their_names
    AFTER INSERT OR UPDATE OF name ON counterparties
    FOR EACH ROW EXECUTE FUNCTION counterparty_alias_gives_way();

-- The counterparty a typed word means: the one typed that way, or the one a
-- counterparty typed that way was merged into. One function, so every path
-- that reads a typed name - a line, a receipt, an entry coming back from the
-- trash - reads it the same way.
CREATE FUNCTION counterparty_typed(owner uuid, typed text) RETURNS uuid AS $$
    SELECT COALESCE(
        (SELECT c.id FROM counterparties c WHERE c.owner_id = owner AND c.key = typed_key(typed)),
        (SELECT a.counterparty_id FROM counterparty_aliases a WHERE a.owner_id = owner AND a.key = typed_key(typed)))
$$ LANGUAGE sql STABLE;

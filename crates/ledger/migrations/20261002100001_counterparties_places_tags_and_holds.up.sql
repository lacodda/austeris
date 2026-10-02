-- What an entry says about itself beyond its lines: who the money went to or
-- came from, where it happened, which of the person's own labels it carries,
-- and whether the bank has posted it yet.
--
-- All four belong to the entry, not to a line. A receipt is one shop however
-- many categories it is split across; a transfer between two of a person's own
-- accounts has no counterparty at all; a payment is held or posted as a whole.

-- Every reference added here is to something of the same person's, and that is
-- held by the database rather than checked by whoever writes: the foreign keys
-- below name the owner as well as the row, so a row of one person's pointing at
-- another person's cannot be stored. These are the keys they point at.
ALTER TABLE categories ADD CONSTRAINT categories_owner_id_key UNIQUE (owner_id, id);
ALTER TABLE entries ADD CONSTRAINT entries_owner_id_key UNIQUE (owner_id, id);

-- Who the money went to or came from: a shop, a service, a person, an employer.
CREATE TYPE counterparty_kind AS ENUM ('shop', 'service', 'person', 'organisation');

-- How a counterparty is named in a typed line: one word, so `@casarica` finds
-- "Casa Rica". Lower case with spaces and the punctuation names carry removed.
-- The characters removed are listed rather than taken as "whatever is not a
-- letter", because which letters a server recognises depends on how it was
-- built. This is the one definition both the stored key and every lookup go
-- through.
CREATE FUNCTION counterparty_key(name text) RETURNS text AS $$
    SELECT regexp_replace(lower(name), '[[:space:]._,''’&/+-]+', '', 'g')
$$ LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE;

CREATE TABLE counterparties (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    name        text        NOT NULL CHECK (btrim(name) <> ''),
    -- Derived from the name and never written on its own: a second copy of a
    -- name that has to agree with it is a copy that one day does not.
    key         text        GENERATED ALWAYS AS (counterparty_key(name)) STORED,
    -- What it is, when the person has said. Not said is a state of its own
    -- rather than a guess: a counterparty created by naming it in a typed line
    -- is not known to be a shop.
    kind        counterparty_kind,
    -- What money spent here is usually for - "this shop is groceries" - so a
    -- typed line naming the counterparty can leave the category out.
    default_category_id uuid,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    -- A name with nothing in it but punctuation could never be typed.
    CONSTRAINT counterparties_key_not_empty CHECK (key <> ''),
    CONSTRAINT counterparties_owner_id_key UNIQUE (owner_id, id),
    -- Deleting the category forgets the default rather than the counterparty.
    CONSTRAINT counterparties_default_category_fkey FOREIGN KEY (owner_id, default_category_id)
        REFERENCES categories (owner_id, id) ON DELETE SET NULL (default_category_id)
);

-- One counterparty per way of typing it: "Casa Rica" and "casa-rica" are one
-- shop, and two rows for it would split what was spent there in two.
CREATE UNIQUE INDEX counterparties_owner_key ON counterparties (owner_id, key);

-- Where it happened: a country, and a city in it when the person said one.
CREATE TABLE places (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- ISO 3166-1 alpha-2, upper case. Which codes exist is checked by the
    -- service; the shape is held here.
    country     char(2)     NOT NULL CHECK (country ~ '^[A-Z]{2}$'),
    city        text        CHECK (city IS NULL OR btrim(city) <> ''),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT places_owner_id_key UNIQUE (owner_id, id)
);

-- A place is found by what it is, so "Asunción" typed twice is one place - and
-- renaming it is one row, not every entry that happened there. The country
-- alone is a place too, distinct from any city in it.
CREATE UNIQUE INDEX places_owner_country_city_key ON places (owner_id, country, lower(COALESCE(city, '')));

-- The person's own labels across categories: `#holiday-2027` on the flights,
-- the hotel and the dinners, whatever each was filed under.
CREATE TABLE tags (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- One token, so every tag can be typed after a `#`. The service holds the
    -- rest of the shape - a letter first, so `#1234` stays an order number.
    name        text        NOT NULL CHECK (name <> '' AND name !~ '[[:space:]#@]'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT tags_owner_id_key UNIQUE (owner_id, id)
);

CREATE UNIQUE INDEX tags_owner_name_key ON tags (owner_id, lower(name));

CREATE TABLE entry_tags (
    owner_id    uuid NOT NULL,
    entry_id    uuid NOT NULL,
    tag_id      uuid NOT NULL,
    PRIMARY KEY (entry_id, tag_id),
    -- A tag goes with its entry, and an entry loses a tag that is deleted:
    -- a label is not a fact about the money.
    CONSTRAINT entry_tags_entry_fkey FOREIGN KEY (owner_id, entry_id) REFERENCES entries (owner_id, id) ON DELETE CASCADE,
    CONSTRAINT entry_tags_tag_fkey FOREIGN KEY (owner_id, tag_id) REFERENCES tags (owner_id, id) ON DELETE CASCADE
);

CREATE INDEX entry_tags_tag_idx ON entry_tags (tag_id);

-- Whether the bank has posted it.
--
-- A card payment leaves the moment it is made and is posted days later, often
-- for a different amount: a hotel's pre-authorisation, a fuel pump's, a
-- charge in another currency converted at the bank's rate on the day it
-- posts. Until then it is held: the account's balance does not show it yet,
-- and the money is not available either.
--
-- Stated rather than read off an empty date, as the side of a line is: a
-- reconciled state arrives with matching against a bank statement, and an
-- entry whose date was lost by mistake must fail rather than read as held.
CREATE TYPE entry_status AS ENUM ('pending', 'cleared');

ALTER TABLE entries
    ADD COLUMN counterparty_id uuid,
    ADD COLUMN place_id uuid,
    ADD COLUMN status entry_status NOT NULL DEFAULT 'cleared',
    -- The day the bank posted it: what a balance as of a day is built on, and
    -- what a statement line is dated by. Kept after clearing so a balance as of
    -- a past day reads the same before and after the entry cleared.
    ADD COLUMN cleared_on date,
    ADD CONSTRAINT entries_counterparty_fkey FOREIGN KEY (owner_id, counterparty_id) REFERENCES counterparties (owner_id, id) ON DELETE RESTRICT,
    ADD CONSTRAINT entries_place_fkey FOREIGN KEY (owner_id, place_id) REFERENCES places (owner_id, id) ON DELETE RESTRICT;

-- Everything recorded until now was posted the day it happened. `entries` has
-- no deferred checks of its own, so filling the column queues nothing.
UPDATE entries SET cleared_on = occurred_on;

ALTER TABLE entries
    ADD CONSTRAINT entries_cleared_when_said CHECK ((status = 'pending') = (cleared_on IS NULL)),
    -- Posted before it happened is a date typed wrong.
    ADD CONSTRAINT entries_cleared_after_it_happened CHECK (cleared_on >= occurred_on);

CREATE INDEX entries_counterparty_idx ON entries (counterparty_id) WHERE counterparty_id IS NOT NULL;
CREATE INDEX entries_place_idx ON entries (place_id) WHERE place_id IS NOT NULL;
CREATE INDEX entries_pending_idx ON entries (owner_id) WHERE status = 'pending';

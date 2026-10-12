-- Templates: buttons that record the same payment again.
--
-- A template is kept by what it points at - an account, a category, a
-- counterparty, a place, tags - rather than as the line a person would type.
-- A category renamed next month is still the one the button means, where a
-- stored line saying `food` would stop working the day "food" became
-- "groceries". Every reference names the owner as well as the row, as an
-- entry's do, so a template of one person's cannot point at another's.

ALTER TABLE accounts ADD CONSTRAINT accounts_owner_id_key UNIQUE (owner_id, id);

CREATE TABLE templates (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- What the button says.
    name        text        NOT NULL CHECK (btrim(name) <> ''),
    account_id  uuid        NOT NULL,
    category_id uuid        NOT NULL,
    -- How much, in `currency`. NULL is a template that asks for the amount
    -- each time: the same shop, a different receipt.
    amount      numeric(38, 18) CHECK (amount IS NULL OR amount > 0),
    -- The account's currency, or the one a payment from it is charged in,
    -- converted at the day's rate when the template is used.
    currency    text        NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    description text        NOT NULL DEFAULT '',
    counterparty_id uuid,
    place_id    uuid,
    -- Each entry it records is held: a fuel pump's pre-authorisation.
    pending     boolean     NOT NULL DEFAULT false,
    -- Where the button sits among the person's others. Not unique: a reorder
    -- rewrites them all in one statement, and a unique index would refuse the
    -- rows passing each other on the way.
    position    integer     NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT templates_owner_id_key UNIQUE (owner_id, id),
    -- A button for an account or a category that no longer exists records
    -- nothing anyone meant, so it goes with them. Neither can be deleted while
    -- entries name it, which is most of the time.
    CONSTRAINT templates_account_fkey FOREIGN KEY (owner_id, account_id) REFERENCES accounts (owner_id, id) ON DELETE CASCADE,
    CONSTRAINT templates_category_fkey FOREIGN KEY (owner_id, category_id) REFERENCES categories (owner_id, id) ON DELETE CASCADE,
    -- Who and where are details: a template outlives forgetting them.
    CONSTRAINT templates_counterparty_fkey FOREIGN KEY (owner_id, counterparty_id)
        REFERENCES counterparties (owner_id, id) ON DELETE SET NULL (counterparty_id),
    CONSTRAINT templates_place_fkey FOREIGN KEY (owner_id, place_id)
        REFERENCES places (owner_id, id) ON DELETE SET NULL (place_id)
);

-- One button per name: two called "Coffee" is a row of buttons nobody can
-- tell apart.
CREATE UNIQUE INDEX templates_owner_name_key ON templates (owner_id, lower(name));
CREATE INDEX templates_owner_position_idx ON templates (owner_id, position);

CREATE TABLE template_tags (
    owner_id    uuid NOT NULL,
    template_id uuid NOT NULL,
    tag_id      uuid NOT NULL,
    PRIMARY KEY (template_id, tag_id),
    CONSTRAINT template_tags_template_fkey FOREIGN KEY (owner_id, template_id) REFERENCES templates (owner_id, id) ON DELETE CASCADE,
    CONSTRAINT template_tags_tag_fkey FOREIGN KEY (owner_id, tag_id) REFERENCES tags (owner_id, id) ON DELETE CASCADE
);

CREATE INDEX template_tags_tag_idx ON template_tags (tag_id);

-- What the same entry a moment ago is looked up by: a person's entries made
-- in the last minute. Partial to the typed ones, which are the only ones the
-- check reads - a module's retries are settled by their idempotency keys.
CREATE INDEX entries_typed_recently_idx ON entries (owner_id, created_at DESC) WHERE idempotency_key IS NULL;

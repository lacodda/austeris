-- What an entry remembers: who changed it and what it was before, the files
-- that prove it, and a way back from deleting it.
--
-- The history is written by the database rather than by the code that
-- usually writes, for the same reason the balance rule is: an entry changed in
-- psql or by a repair script is changed all the same, and a history that only
-- the service keeps is a history with holes exactly where something unusual
-- happened.
--
-- The trash is built on the history rather than beside it. Deleting an entry
-- really deletes it - its lines leave every balance, total and list, and none
-- of them has a filter to forget - and what it was is the last thing its
-- history remembers. Restoring it writes that back.

-- Files kept with an entry: a photo of the receipt, the bank's PDF, the
-- receipt's XML.
--
-- In the database rather than in files beside it: an attachment then goes to
-- the trash and comes back in the same transaction as its entry, a copy of the
-- database taken before an upgrade carries it, and there is no folder to back
-- up separately or to fall out of step with the rows that name it.
CREATE TABLE attachments (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id    uuid        NOT NULL,
    -- The entry it is kept with. Not a foreign key, on purpose: an entry in
    -- the trash is not in `entries`, and its attachments wait for it there
    -- rather than going with the row - restoring the entry restores them
    -- without anything being copied. That they name a live entry of the same
    -- person when they arrive is held by a trigger below.
    entry_id    uuid        NOT NULL,
    -- The file's name as it was uploaded, for showing and for saving it again.
    name        text        NOT NULL CHECK (btrim(name) <> ''),
    -- What the bytes are, as read from the bytes rather than taken from the
    -- uploader's word for them.
    media_type  text        NOT NULL,
    size        bigint      NOT NULL CHECK (size > 0),
    sha256      bytea       NOT NULL CHECK (length(sha256) = 32),
    content     bytea       NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    -- Taken off its entry, and in the trash since then.
    removed_at  timestamptz,
    CONSTRAINT attachments_size_is_the_content CHECK (size = length(content)),
    CONSTRAINT attachments_owner_id_key UNIQUE (owner_id, id)
);

-- Photos and PDFs are compressed already; trying again costs a Raspberry Pi
-- time for nothing. Kept out of line, so reading what an entry has attached
-- never reads the files.
ALTER TABLE attachments ALTER COLUMN content SET STORAGE EXTERNAL;

CREATE INDEX attachments_entry_idx ON attachments (entry_id);
-- The same file twice on one entry is one receipt photographed once and
-- uploaded twice.
CREATE UNIQUE INDEX attachments_entry_content_key ON attachments (entry_id, sha256) WHERE removed_at IS NULL;
CREATE INDEX attachments_removed_idx ON attachments (owner_id, removed_at) WHERE removed_at IS NOT NULL;

-- An attachment arrives on - or comes back to - an entry that is this
-- person's and is not in the trash.
CREATE FUNCTION attachment_on_live_entry() RETURNS trigger AS $$
BEGIN
    IF NEW.removed_at IS NULL
       AND NOT EXISTS (SELECT 1 FROM entries WHERE id = NEW.entry_id AND owner_id = NEW.owner_id) THEN
        RAISE EXCEPTION 'entry % is not one of this person''s entries, or it is in the trash', NEW.entry_id
            USING ERRCODE = 'foreign_key_violation';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER attachments_on_live_entry
    BEFORE INSERT OR UPDATE OF entry_id, owner_id, removed_at ON attachments
    FOR EACH ROW EXECUTE FUNCTION attachment_on_live_entry();

-- What happened to an entry, as one word.
CREATE TYPE revision_action AS ENUM ('created', 'changed', 'deleted', 'restored');

-- Who did it: a person through the gateway, a module posting for them, or
-- something that wrote to the database without going through austeris at all.
CREATE TYPE revision_actor AS ENUM ('person', 'module', 'outside');

-- An entry as it was: everything a person reads on it, with the ids that say
-- which account, category, tag or counterparty each part was. Names are kept
-- beside the ids so a counterparty deleted since is still named.
--
-- The one definition of an entry's state: the history compares these, the
-- trash restores from them, and the service reads the current one through
-- this same function rather than assembling a second copy of the shape.
CREATE FUNCTION entry_snapshot(subject uuid) RETURNS jsonb AS $$
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

-- One row per entry per transaction that touched it.
--
-- What is kept is the entry as it was *before* the transaction - nothing for
-- one that created it - so the state after a revision is the state before the
-- next one, or the entry as it is now. One transaction is one revision however
-- many rows it wrote: posting a hold for another amount rewrites every line
-- and the header, and a person did one thing.
CREATE TABLE entry_revisions (
    -- The order they happened in. A uuid is not an order, and a timestamp is
    -- shared by everything one transaction does.
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    owner_id     uuid            NOT NULL,
    -- Not a foreign key: the history of a deleted entry is what the trash is.
    entry_id     uuid            NOT NULL,
    action       revision_action NOT NULL,
    actor        revision_actor  NOT NULL,
    actor_person uuid,
    -- The module's name, when it gave one.
    actor_module text,
    at           timestamptz     NOT NULL DEFAULT now(),
    txid         xid8            NOT NULL DEFAULT pg_current_xact_id(),
    before       jsonb,
    CONSTRAINT entry_revisions_once_per_transaction UNIQUE (entry_id, txid),
    CONSTRAINT entry_revisions_person_when_one CHECK ((actor = 'person') = (actor_person IS NOT NULL)),
    CONSTRAINT entry_revisions_module_when_one CHECK (actor_module IS NULL OR actor = 'module'),
    -- Something was there before every change and every deletion, and nothing
    -- before a creation.
    CONSTRAINT entry_revisions_before_when_there_was_one CHECK ((before IS NULL) = (action IN ('created', 'restored')))
);

CREATE INDEX entry_revisions_entry_idx ON entry_revisions (entry_id, id);
CREATE INDEX entry_revisions_deleted_idx ON entry_revisions (owner_id, at) WHERE action = 'deleted';

-- Records that the current transaction touches an entry, once.
--
-- Called before the row changes, so the first call of a transaction still
-- sees the entry as it was. Who is read from `austeris.actor`, which the
-- service sets for its own transaction: `person:<id>` or `module:<name>`.
-- Nothing set is a write from outside austeris, and is recorded as one -
-- unless the connection is austeris's own, which means a path of the service
-- that forgot to say on whose behalf it writes. That is refused, so the path
-- fails where it is tested rather than leaving a history that blames nobody.
CREATE FUNCTION entry_touched(subject uuid, act revision_action, owner uuid DEFAULT NULL) RETURNS void AS $$
DECLARE
    said text := current_setting('austeris.actor', true);
    present entry_revisions%ROWTYPE;
    whose uuid := owner;
    who revision_actor;
    person uuid;
    module text;
BEGIN
    SELECT * INTO present FROM entry_revisions WHERE entry_id = subject AND txid = pg_current_xact_id();
    IF FOUND THEN
        IF act = 'deleted' AND present.action IN ('created', 'restored') THEN
            -- Made and unmade in one transaction: nothing happened to it that
            -- anyone could have seen.
            DELETE FROM entry_revisions WHERE id = present.id;
        ELSIF act = 'deleted' THEN
            UPDATE entry_revisions SET action = 'deleted' WHERE id = present.id;
        ELSIF act IN ('created', 'restored') AND present.action = 'deleted' THEN
            -- Deleted and written again in one transaction is a change.
            UPDATE entry_revisions SET action = 'changed' WHERE id = present.id;
        END IF;
        RETURN;
    END IF;

    IF act NOT IN ('created', 'restored') THEN
        SELECT owner_id INTO whose FROM entries WHERE id = subject;
        -- A part of an entry already gone - the lines a deletion cascades to
        -- after the entry itself - has nothing left to remember.
        IF NOT FOUND THEN
            RETURN;
        END IF;
    END IF;

    IF said IS NULL OR said = '' THEN
        IF current_setting('application_name', true) = 'austeris' THEN
            RAISE EXCEPTION 'austeris changed entry % without saying on whose behalf', subject;
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

    INSERT INTO entry_revisions (owner_id, entry_id, action, actor, actor_person, actor_module, before)
    VALUES (whose, subject, act, who, person, module,
            CASE WHEN act IN ('created', 'restored') THEN NULL ELSE entry_snapshot(subject) END);
END;
$$ LANGUAGE plpgsql;

CREATE FUNCTION entries_remembered() RETURNS trigger AS $$
DECLARE
    last revision_action;
BEGIN
    IF TG_OP = 'INSERT' THEN
        -- An entry whose history ends in its deletion is coming back.
        SELECT action INTO last FROM entry_revisions WHERE entry_id = NEW.id ORDER BY id DESC LIMIT 1;
        PERFORM entry_touched(NEW.id, CASE WHEN last = 'deleted' THEN 'restored' ELSE 'created' END::revision_action, NEW.owner_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        PERFORM entry_touched(OLD.id, 'changed');
        RETURN NEW;
    END IF;
    PERFORM entry_touched(OLD.id, 'deleted');
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entries_remembered
    BEFORE INSERT OR UPDATE OR DELETE ON entries
    FOR EACH ROW EXECUTE FUNCTION entries_remembered();

-- A line, a tag on it or a file kept with it changing is the entry changing.
CREATE FUNCTION entry_part_remembered() RETURNS trigger AS $$
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') THEN
        PERFORM entry_touched(OLD.entry_id, 'changed');
    END IF;
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        PERFORM entry_touched(NEW.entry_id, 'changed');
        RETURN NEW;
    END IF;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER entry_lines_remembered
    BEFORE INSERT OR UPDATE OR DELETE ON entry_lines
    FOR EACH ROW EXECUTE FUNCTION entry_part_remembered();

CREATE TRIGGER entry_tags_remembered
    BEFORE INSERT OR UPDATE OR DELETE ON entry_tags
    FOR EACH ROW EXECUTE FUNCTION entry_part_remembered();

-- After the attachment's own check, so a refused one leaves no revision.
CREATE TRIGGER attachments_remembered
    BEFORE INSERT OR UPDATE ON attachments
    FOR EACH ROW EXECUTE FUNCTION entry_part_remembered();

-- Erasing a file that was already taken off is not a change to the entry: it
-- left the entry when it was taken off. So the hourly erasing of the trash
-- touches no entry's history - and would be refused if it ever reached a file
-- still on one.
CREATE TRIGGER attachments_deletion_remembered
    BEFORE DELETE ON attachments
    FOR EACH ROW WHEN (OLD.removed_at IS NULL)
    EXECUTE FUNCTION entry_part_remembered();

-- A tag deleted takes itself off every entry carrying it, and by the time that
-- cascade reaches `entry_tags` the tag is gone and the entries would be
-- remembered without it. So they are remembered here, while it is still there.
CREATE FUNCTION tag_removal_remembered() RETURNS trigger AS $$
DECLARE
    tagged uuid;
BEGIN
    FOR tagged IN SELECT entry_id FROM entry_tags WHERE tag_id = OLD.id LOOP
        PERFORM entry_touched(tagged, 'changed');
    END LOOP;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER tags_removal_remembered
    BEFORE DELETE ON tags
    FOR EACH ROW EXECUTE FUNCTION tag_removal_remembered();

-- A change that changed nothing is not one: replacing an entry's tags with the
-- same tags deletes and writes them again, and saving a form untouched writes
-- the same description back. Settled at COMMIT, when the transaction's last
-- word on the entry is known.
CREATE FUNCTION revision_says_something() RETURNS trigger AS $$
DECLARE
    kept entry_revisions%ROWTYPE;
BEGIN
    -- Read again: the row may have become a deletion, or gone, since it was
    -- written.
    SELECT * INTO kept FROM entry_revisions WHERE id = NEW.id;
    IF FOUND AND kept.action = 'changed' AND kept.before = entry_snapshot(kept.entry_id) THEN
        DELETE FROM entry_revisions WHERE id = kept.id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE CONSTRAINT TRIGGER entry_revisions_say_something
    AFTER INSERT ON entry_revisions
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (NEW.action = 'changed')
    EXECUTE FUNCTION revision_says_something();

-- The entries already recorded begin their history with being recorded. Until
-- now only two things wrote to a person's books: the person, through the
-- gateway, and a module, which signs what it posts - so who made each one is
-- known. What changed in them before today is not, and is not invented.
INSERT INTO entry_revisions (owner_id, entry_id, action, actor, actor_person, actor_module, at, before)
SELECT owner_id, id, 'created',
       CASE WHEN source IS NULL THEN 'person' ELSE 'module' END::revision_actor,
       CASE WHEN source IS NULL THEN owner_id END,
       source,
       created_at,
       NULL
  FROM entries;

-- The documents a person hands austeris to be read into the books: a receipt
-- uploaded, or dropped in a folder.
--
-- A document is kept here only for as long as it is not in the books. Once its
-- entry is written the file lives with the entry, in the ledger, and what stays
-- here is the trace: that this file arrived, what it was read as, and which
-- entry it became - so the same file arriving again is answered rather than
-- booked twice.

-- Where a document stands.
--
-- `waiting`: read, and not in the books yet - the person has to say which
-- account paid it or what it was for, or the ledger could not be reached.
-- `booked`: in the books, as `entry_id`. `refused`: not something that can be
-- read into the books, and why.
CREATE TYPE document_state AS ENUM ('waiting', 'booked', 'refused');

-- How it arrived.
CREATE TYPE document_origin AS ENUM ('upload', 'folder');

CREATE TABLE documents (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id      uuid            NOT NULL,
    -- The file's name as it arrived, for showing and for keeping it with the
    -- entry under the same name.
    name          text            NOT NULL CHECK (btrim(name) <> ''),
    size          bigint          NOT NULL CHECK (size >= 0),
    sha256        bytea           NOT NULL CHECK (length(sha256) = 32),
    -- The file itself, while it waits: booking it later writes it to the
    -- entry. Never kept once it is booked (the entry has it) or refused
    -- (there is nothing to do with it).
    content       bytea,
    origin        document_origin NOT NULL,
    state         document_state  NOT NULL,
    -- What it was read as: the receipt, as the screen shows it while it
    -- waits and as it was when it was booked.
    receipt       jsonb,
    -- The receipt's own code - SIFEN's CDC - so one receipt arriving as two
    -- different files is still one receipt.
    code          text,
    -- What the person said about it: the account it was paid from and what
    -- it was for. Kept while it waits, so saying one does not lose the other.
    account_id    uuid,
    category_id   uuid,
    -- The account the folder it arrived in is named after, when the person
    -- has none by that name - said back so the folder can be renamed.
    account_name  text,
    -- What it waits for: `account`, `category`, or both.
    needs         text[]          NOT NULL DEFAULT '{}' CHECK (needs <@ ARRAY['account', 'category']::text[]),
    -- Why it waits or was refused: `{code, params, message}` - a code a screen
    -- says in its own language, the facts its sentence is made from, and the
    -- sentence in English for a terminal.
    reason        jsonb CHECK (reason IS NULL OR (reason ? 'code' AND reason ? 'params' AND reason ? 'message')),
    -- The entry it became.
    entry_id      uuid,
    received_at   timestamptz     NOT NULL DEFAULT now(),
    booked_at     timestamptz,
    -- A document in the books names its entry, and only then.
    CONSTRAINT documents_entry_when_booked CHECK ((state = 'booked') = (entry_id IS NOT NULL AND booked_at IS NOT NULL)),
    -- The file is here exactly while the document waits for it to be booked.
    CONSTRAINT documents_content_while_waiting CHECK ((state = 'waiting') = (content IS NOT NULL)),
    -- A refusal says why; a booked document has nothing left to explain.
    CONSTRAINT documents_reason_when_refused CHECK (state <> 'refused' OR reason IS NOT NULL),
    CONSTRAINT documents_nothing_needed_once_booked CHECK (state <> 'booked' OR needs = '{}')
);

-- XML is small and compresses well, but so would nothing else that waits here;
-- kept out of line, so listing what waits never reads the files.
ALTER TABLE documents ALTER COLUMN content SET STORAGE EXTERNAL;

-- The same file twice is one document: answered as it stands, not read again.
CREATE UNIQUE INDEX documents_owner_sha256_key ON documents (owner_id, sha256);
-- The same receipt twice, however its file differs, is one document too.
CREATE UNIQUE INDEX documents_owner_code_key ON documents (owner_id, code) WHERE code IS NOT NULL;
CREATE INDEX documents_owner_received_idx ON documents (owner_id, received_at DESC);
CREATE INDEX documents_waiting_idx ON documents (owner_id) WHERE state = 'waiting';

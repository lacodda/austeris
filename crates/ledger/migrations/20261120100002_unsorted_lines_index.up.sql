-- The drafts are what a person comes back to, and how many there are is on
-- every screen: read often, and few. A migration of its own because the side
-- it names cannot be named in the transaction that added it.
CREATE INDEX entry_lines_unsorted_idx ON entry_lines (entry_id) WHERE side = 'unsorted';

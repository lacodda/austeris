-- The previous schema has no word for money not yet sorted. Rolling back over
-- a draft would leave an entry whose other side is nothing it can name, so it
-- is refused rather than done quietly. A value cannot leave an enum: the word
-- stays in the type, unused.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM entry_lines WHERE side::text = 'unsorted')
       OR EXISTS (SELECT 1 FROM entry_revisions WHERE jsonb_path_exists(before, '$.lines[*] ? (@.side == "unsorted")')) THEN
        RAISE EXCEPTION 'entries are drafts, not yet sorted, which the previous schema cannot express; sort or erase them before rolling back';
    END IF;
END;
$$;

-- The previous schema has no word for a file kept with an entry, for an
-- entry's history or for the trash. Rolling back over any of them would drop a
-- receipt, a record of what an entry was, or a deleted entry that could still
-- come back - so it is refused rather than done quietly. The history every
-- entry begins with, its being recorded, is what the previous schema already
-- says on the entry itself, and goes without loss.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM attachments) THEN
        RAISE EXCEPTION 'files are attached to entries, and the previous schema would drop them; remove them and empty the trash before rolling back';
    END IF;
    IF EXISTS (SELECT 1 FROM entry_revisions WHERE action <> 'created') THEN
        RAISE EXCEPTION 'entries have a history of changes or are in the trash, and the previous schema would drop it';
    END IF;
END;
$$;

DROP TRIGGER tags_removal_remembered ON tags;
DROP TRIGGER attachments_deletion_remembered ON attachments;
DROP TRIGGER attachments_remembered ON attachments;
DROP TRIGGER entry_tags_remembered ON entry_tags;
DROP TRIGGER entry_lines_remembered ON entry_lines;
DROP TRIGGER entries_remembered ON entries;
DROP FUNCTION tag_removal_remembered();
DROP FUNCTION entry_part_remembered();
DROP FUNCTION entries_remembered();
DROP FUNCTION entry_touched(uuid, revision_action, uuid);
DROP TABLE entry_revisions;
DROP FUNCTION revision_says_something();
DROP FUNCTION entry_snapshot(uuid);
DROP TYPE revision_actor;
DROP TYPE revision_action;
DROP TABLE attachments;
DROP FUNCTION attachment_on_live_entry();

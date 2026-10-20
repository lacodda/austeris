-- A counterparty's tax number: what the receipts it issues are signed with.
--
-- A shop is printed on its receipts under its legal name ("RETAIL S.A."), its
-- trade name ("Superseis") or both, and neither is how the person typed it the
-- first time. Its tax number is the one thing every receipt from it carries the
-- same way, so a receipt finds its counterparty by it - and a counterparty
-- found once by name keeps the number for every receipt after.
--
-- Written the way the issuing country writes it, separators and all: a
-- Paraguayan RUC is `80012345-6`, the number and its check digit. Only its
-- shape is held here; which shapes a country uses is the reader's business.
ALTER TABLE counterparties
    ADD COLUMN tax_id text CHECK (tax_id IS NULL OR tax_id ~ '^[0-9A-Z]+(-[0-9A-Z]+)*$');

-- One counterparty per tax number: two rows for one shop would split what was
-- spent there in two, which is what the key of its name already prevents.
CREATE UNIQUE INDEX counterparties_owner_tax_id_key ON counterparties (owner_id, tax_id) WHERE tax_id IS NOT NULL;

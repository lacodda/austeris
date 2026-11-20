-- The previous schema reads a typed name only as a counterparty's own. What a
-- merged counterparty was typed as is forgotten; the merge itself stays, as
-- its entries say.
DROP FUNCTION counterparty_typed(uuid, text);
DROP TRIGGER counterparties_take_their_names ON counterparties;
DROP FUNCTION counterparty_alias_gives_way();
DROP TABLE counterparty_aliases;

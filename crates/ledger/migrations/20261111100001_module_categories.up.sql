-- Categories a module files money under, by a name of its own.
--
-- A trade's fee is money spent - what a broker or an exchange took - and a
-- person wants to know how much of it went in a year. The portfolio knows the
-- fee; the ledger knows the categories. Asking a person to make a category
-- before their first trade, and to point the portfolio at it, is a form in the
-- way of a sentence - and the ledger cannot keep a purpose of its own for every
-- module's money without learning what the modules are (ADR 0001).
--
-- So a module names the category by a key of its own - the portfolio's `fees`
-- - and the ledger keeps, per person, which of their categories that is. It is
-- made on first use at the root under the name the module suggests, or a root
-- category of the person's by that name is taken for it; after that it is found
-- by the module and the key, not by its name, so the person may rename it or
-- move it and it stays the one money is filed into. Like the ledger's own
-- categories (`purpose`), and never both.
ALTER TABLE categories
    ADD COLUMN module     text CHECK (module IS NULL OR (module = btrim(module) AND module <> '')),
    ADD COLUMN module_key text CHECK (module_key IS NULL OR module_key ~ '^[a-z][a-z0-9_]*$');

ALTER TABLE categories ADD CONSTRAINT categories_module_names_a_key CHECK ((module IS NULL) = (module_key IS NULL));
ALTER TABLE categories ADD CONSTRAINT categories_kept_by_one CHECK (module IS NULL OR purpose IS NULL);

CREATE UNIQUE INDEX categories_owner_module_key ON categories (owner_id, module, module_key) WHERE module IS NOT NULL;

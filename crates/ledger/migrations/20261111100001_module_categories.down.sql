-- The previous schema has no word for a category a module files under. The
-- categories themselves stay - they are the person's, with their money in them
-- - and only which module looked for which by what key is forgotten. Applied
-- again, a module takes the root category by the name it suggests for its own
-- once more, so nothing is made twice unless the person renamed it meanwhile.
DROP INDEX categories_owner_module_key;
ALTER TABLE categories DROP CONSTRAINT categories_kept_by_one;
ALTER TABLE categories DROP CONSTRAINT categories_module_names_a_key;
ALTER TABLE categories DROP COLUMN module_key;
ALTER TABLE categories DROP COLUMN module;

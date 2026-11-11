-- A category is typed in a line the way a counterparty is: one word. A line is
-- read word by word, so "Eating out" could not be named in it at all; with the
-- same key `@casarica` finds "Casa Rica" by, `eatingout` and `eating-out` find
-- it, and `food/eating-out` finds it under "Food".
--
-- One rule for every name a line types, so the function that holds it is
-- called for what it is now. The counterparties' stored key goes on calling
-- it: a generated column names its function by identity, not by name.
ALTER FUNCTION counterparty_key(text) RENAME TO typed_key;

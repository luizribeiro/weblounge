-- Service bots are accounts the network itself runs, such as bender, the
-- #ops bot. They stay under a member's name, who answers for them, but
-- don't take any of the places that member has for bots of their own.

ALTER TABLE bots ADD COLUMN service BOOLEAN NOT NULL DEFAULT FALSE;

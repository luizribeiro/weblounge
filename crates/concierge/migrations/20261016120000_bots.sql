-- Ergo accounts members own and run programs on. Ergo can't tell them from
-- members' accounts, so this is the only record of which accounts are bots.
-- A deleted bot's row stays, with removed_at, so its name isn't handed out
-- again once Ergo has erased the account.

CREATE TABLE bots (
    account TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,
    owner TEXT NOT NULL REFERENCES members (account),
    note TEXT,
    created_at INTEGER NOT NULL,
    removed_at INTEGER
);

CREATE INDEX bots_by_owner ON bots (owner);

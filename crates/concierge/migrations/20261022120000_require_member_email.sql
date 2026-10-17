-- Every member gives an email at signup, and verification mails it, so
-- only a removed member's row may lack one. SQLite can't add a CHECK with
-- ALTER TABLE: the table is rebuilt the way SQLite's ALTER TABLE docs
-- describe. Concierge runs migrations with foreign keys off, so the tables
-- that refer to members carry over to the new one.

CREATE TABLE new_members (
    -- Ergo's canonical spelling of the account name, which is also the nick.
    account TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,
    email TEXT,
    email_verified_at INTEGER,
    news_opt_in BOOLEAN NOT NULL DEFAULT FALSE,
    invited_by TEXT REFERENCES members (account),
    joined_at INTEGER NOT NULL,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    -- A removed member stays in the invite tree, so the people they invited
    -- stay attached: the row and its name remain, everything else goes.
    removed_at INTEGER,
    CHECK (removed_at IS NOT NULL OR email IS NOT NULL),
    CHECK (
        removed_at IS NULL OR (
            email IS NULL AND email_verified_at IS NULL AND NOT news_opt_in
            AND NOT is_admin
        )
    )
);

INSERT INTO new_members
    (account, email, email_verified_at, news_opt_in, invited_by, joined_at, is_admin, removed_at)
SELECT account, email, email_verified_at, news_opt_in, invited_by, joined_at, is_admin, removed_at
FROM members;

DROP TABLE members;

ALTER TABLE new_members RENAME TO members;

CREATE INDEX members_by_inviter ON members (invited_by);

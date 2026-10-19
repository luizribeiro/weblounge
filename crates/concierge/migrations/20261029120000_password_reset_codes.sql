-- The codes in the links that reset members' passwords, kept by their
-- SHA-256 like verification codes. A code works once, and only while the
-- address it went to is still its member's confirmed one. A member has
-- one at most: a new link replaces the last.

CREATE TABLE password_reset_codes (
    code_hash BLOB NOT NULL PRIMARY KEY,
    account TEXT NOT NULL UNIQUE REFERENCES members (account),
    email TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

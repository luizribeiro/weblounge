-- The codes in the links that verify members' emails, kept by their
-- SHA-256 like invites. A code verifies only the address it was sent to,
-- and only while it's its member's newest: a replaced code stays until it
-- expires, so its link can say a newer email went out.

CREATE TABLE verification_codes (
    code_hash BLOB NOT NULL PRIMARY KEY,
    account TEXT NOT NULL REFERENCES members (account),
    email TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    replaced_at INTEGER
) WITHOUT ROWID;

CREATE INDEX verification_codes_by_account ON verification_codes (account);

CREATE UNIQUE INDEX verification_codes_live_by_account ON verification_codes (account)
WHERE replaced_at IS NULL;

-- A member's week to verify runs from the first verification email.
ALTER TABLE members ADD COLUMN verification_first_sent_at INTEGER;
ALTER TABLE members ADD COLUMN verification_reminded_at INTEGER;

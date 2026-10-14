-- Invites are kept by the SHA-256 of their code, never the code: links are
-- shown once, so nothing needs a code back. SQLite can't hash, so rows
-- carry their codes over as they are, and concierge hashes them when it
-- opens the database.

CREATE TABLE new_invites (
    code_hash BLOB NOT NULL PRIMARY KEY,
    -- NULL for root invites, which admins mint on the command line.
    created_by TEXT REFERENCES members (account),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    claimed_at INTEGER,
    redeemed_by TEXT UNIQUE REFERENCES members (account),
    redeemed_at INTEGER,
    revoked_at INTEGER,
    CHECK ((redeemed_by IS NULL) = (redeemed_at IS NULL))
);

INSERT INTO new_invites
    (code_hash, created_by, created_at, expires_at, claimed_at, redeemed_by, redeemed_at, revoked_at)
SELECT CAST(code AS BLOB), created_by, created_at, expires_at, claimed_at, redeemed_by, redeemed_at, revoked_at
FROM invites;

DROP TABLE invites;

ALTER TABLE new_invites RENAME TO invites;

CREATE INDEX invites_by_creator ON invites (created_by);

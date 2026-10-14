-- Times are Unix seconds.

CREATE TABLE members (
    -- Ergo's canonical spelling of the account name, which is also the nick.
    account TEXT NOT NULL PRIMARY KEY COLLATE NOCASE,
    email TEXT,
    email_verified_at INTEGER,
    news_opt_in BOOLEAN NOT NULL DEFAULT FALSE,
    invited_by TEXT REFERENCES members (account),
    joined_at INTEGER NOT NULL,
    invite_quota INTEGER NOT NULL,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    -- A removed member stays in the invite tree, so the people they invited
    -- stay attached: the row and its name remain, everything else goes.
    removed_at INTEGER,
    CHECK (
        removed_at IS NULL OR (
            email IS NULL AND email_verified_at IS NULL AND NOT news_opt_in
            AND invite_quota = 0 AND NOT is_admin
        )
    )
);

CREATE INDEX members_by_inviter ON members (invited_by);

CREATE TABLE invites (
    code TEXT NOT NULL PRIMARY KEY,
    -- NULL for root invites, which admins mint on the command line.
    created_by TEXT REFERENCES members (account),
    note TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    claimed_at INTEGER,
    redeemed_by TEXT UNIQUE REFERENCES members (account),
    redeemed_at INTEGER,
    revoked_at INTEGER,
    CHECK ((redeemed_by IS NULL) = (redeemed_at IS NULL))
);

CREATE INDEX invites_by_creator ON invites (created_by);

CREATE TABLE sessions (
    token_hash BLOB NOT NULL PRIMARY KEY,
    account TEXT NOT NULL REFERENCES members (account),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX sessions_by_account ON sessions (account);

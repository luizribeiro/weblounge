-- The web client's authorization codes and access tokens, stored as keyed
-- hashes like session tokens. Each belongs to the session that approved
-- it and goes when that session ends.

CREATE TABLE oauth_codes (
    code_hash BLOB NOT NULL PRIMARY KEY,
    session_hash BLOB NOT NULL REFERENCES sessions (token_hash) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX oauth_codes_by_session ON oauth_codes (session_hash);

CREATE TABLE oauth_tokens (
    token_hash BLOB NOT NULL PRIMARY KEY,
    session_hash BLOB NOT NULL REFERENCES sessions (token_hash) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX oauth_tokens_by_session ON oauth_tokens (session_hash);

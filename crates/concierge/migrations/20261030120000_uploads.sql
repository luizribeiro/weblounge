-- The images members and their bots upload, which the scrubber stores as
-- files named by their ids. account is who uploaded one, the member or one
-- of their bots, and owner the member it counts against. Nothing here says
-- where an upload came from.

CREATE TABLE uploads (
    id TEXT NOT NULL PRIMARY KEY,
    account TEXT NOT NULL COLLATE NOCASE,
    owner TEXT NOT NULL COLLATE NOCASE REFERENCES members (account),
    content_type TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX uploads_by_owner ON uploads (owner, created_at);
CREATE INDEX uploads_by_account ON uploads (account);
CREATE INDEX uploads_by_age ON uploads (created_at);

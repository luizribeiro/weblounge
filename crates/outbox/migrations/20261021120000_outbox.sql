-- Plain-text mail waiting for concierge-mailer, which deletes each message
-- once it's sent or given up on, so addresses don't linger here. It's a
-- database of its own, apart from concierge.db, so the mailer reads
-- nothing else concierge keeps.

CREATE TABLE outbox (
    id INTEGER PRIMARY KEY,
    -- The member it's for, whose removal takes it too; NULL for mail that's
    -- for no member, like an admin's test message.
    account TEXT,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL
);

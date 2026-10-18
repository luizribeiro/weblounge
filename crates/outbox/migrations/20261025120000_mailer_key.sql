-- The public half of the age key concierge-mailer generates each time it
-- starts, and never writes down: concierge seals mail that carries a
-- secret to it, so what reaches the disk is nothing anyone can open once
-- that mailer is gone. One row at most.

CREATE TABLE mailer_key (
    only INTEGER PRIMARY KEY CHECK (only = 1),
    -- An age X25519 recipient, age1...
    public_key TEXT NOT NULL
);

-- A message's body is plain text, or sealed to the mailer's key: concierge
-- seals what carries a secret, which only the mailer that published the
-- key can open. Ids now never come back once deleted, so the one the
-- mailer's log names a message by names no other.

CREATE TABLE new_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- The member it's for, whose removal takes it too; NULL for mail that's
    -- for no member, like an admin's test message.
    account TEXT,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    -- Plain text, or NULL when sealed.
    body TEXT,
    -- The mailer_key.public_key a sealed body was sealed to, and the age
    -- ciphertext; NULL when plain.
    sealed_to TEXT,
    sealed_body BLOB,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    CHECK ((body IS NULL) = (sealed_to IS NOT NULL)),
    CHECK ((sealed_to IS NULL) = (sealed_body IS NULL))
);

INSERT INTO new_outbox (id, account, recipient, subject, body, created_at, attempts, next_attempt_at)
SELECT id, account, recipient, subject, body, created_at, attempts, next_attempt_at FROM outbox;

DROP TABLE outbox;

ALTER TABLE new_outbox RENAME TO outbox;

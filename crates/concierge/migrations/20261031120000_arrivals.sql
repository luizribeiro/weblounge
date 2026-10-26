-- Newcomers the greeter welcomes: one row per redeemed invite, written with
-- the member, and when each of its steps happened. arrived_at is when the
-- newcomer was first seen with a session attached, and announce_msgid the
-- welcome line's id in #lounge. Members from before this table are never
-- welcomed, so nothing is backfilled.

CREATE TABLE arrivals (
    account TEXT NOT NULL PRIMARY KEY COLLATE NOCASE REFERENCES members (account),
    arrived_at INTEGER,
    announced_at INTEGER,
    announce_msgid TEXT,
    inviter_told_at INTEGER,
    nudged_at INTEGER,
    oriented_at INTEGER
) WITHOUT ROWID;

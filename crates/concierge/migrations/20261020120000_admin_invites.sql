-- A root invite can make whoever redeems it an admin, which is how a new
-- network gets its first one.
ALTER TABLE invites ADD COLUMN makes_admin BOOLEAN NOT NULL DEFAULT FALSE;

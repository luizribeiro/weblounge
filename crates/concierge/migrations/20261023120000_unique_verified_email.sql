-- A verified address belongs to one member, whatever its case. Unverified
-- ones may repeat, since anyone can type anyone's address; verifying
-- settles whose it is. Addresses are compared as typed otherwise, Gmail's
-- dots and +tags included. SQLite's lower() folds ASCII letters only.
CREATE UNIQUE INDEX members_by_verified_email ON members (lower(email))
WHERE email_verified_at IS NOT NULL;

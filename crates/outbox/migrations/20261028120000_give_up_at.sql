-- When the mailer gives up on a sealed message, if that's earlier than it
-- gives up on any: the link it carries stops working then, and delivering
-- it later helps nobody. NULL for mail that waits as long as any may.

ALTER TABLE outbox ADD COLUMN give_up_at INTEGER;

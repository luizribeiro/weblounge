-- bender, the network's #ops bot in services/bender.nix, is a service bot.
-- Its bot ensure --service runs only before louro starts, which a deploy
-- doesn't do, so its existing row is flagged here. Bots' names are unique,
-- and this one is the network's.

UPDATE bots SET service = TRUE WHERE account = 'bender';

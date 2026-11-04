-- bender, the #ops bot in services/bender.nix, is a house account now:
-- the network's own, whose name concierge reserves from the host's
-- configuration rather than a row here. Its row from when it was luiz's
-- bot goes, so it stops counting toward his bots and ensure-house-account,
-- which refuses a name a bot holds, takes it over.

DELETE FROM bots WHERE account = 'bender';

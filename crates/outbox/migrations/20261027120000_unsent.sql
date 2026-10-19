-- The ids of sealed messages deleted unsent, as no mailer that could open
-- them was left. Concierge counted each against its member's limits on
-- links, and takes these to give those links back.

CREATE TABLE unsent (
    id INTEGER PRIMARY KEY
);

-- The Monday reminders the greeter sent to post the week's thread in
-- #lounge, one a week, under that Monday's date, so a restart never sends
-- one twice.

CREATE TABLE weekly_reminders (
    week TEXT NOT NULL PRIMARY KEY,
    sent_at INTEGER NOT NULL
) WITHOUT ROWID;

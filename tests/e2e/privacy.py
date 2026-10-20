# One subtest per promise on concierge's privacy page, under its headings.
# "Not in the logs" is checked after every VM test, in lib/epilogue.py.
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, mail, subtest
    from helpers import (
        Account,
        Browser,
        concierge_admin,
        concierge_sql,
        files_containing,
        irc,
        irc_command,
        members,
        outcome,
        patterns,
        register,
        wait_for_empty_outbox,
        wait_for_mail,
    )

grace: Account = {"account": "grace", "password": "willow-harbor-58"}
graces_browser = Browser("grace")
email = "grace@example.org"

credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(8089)
mail.wait_for_unit("mailpit-test.service")
assert graces_browser.sign_up(concierge_admin("invite", "create", "--root"), **grace) == "200 "

with subtest("stored: concierge's database has the columns the page was written against"):
    # A new column is something new we store: say so on the page, then add it here.
    columns = concierge_sql(
        "SELECT t.name || '.' || c.name FROM sqlite_schema t, pragma_table_info(t.name) c"
        " WHERE t.type = 'table' AND t.name NOT LIKE '\\_%' ESCAPE '\\' ORDER BY 1"
    ).split()
    assert columns == [
        "bots.account",
        "bots.created_at",
        "bots.note",
        "bots.owner",
        "bots.removed_at",
        "invites.claimed_at",
        "invites.code_hash",
        "invites.created_at",
        "invites.created_by",
        "invites.expires_at",
        "invites.makes_admin",
        "invites.redeemed_at",
        "invites.redeemed_by",
        "invites.revoked_at",
        "members.account",
        "members.email",
        "members.email_verified_at",
        "members.invited_by",
        "members.is_admin",
        "members.joined_at",
        "members.news_opt_in",
        "members.removed_at",
        "members.verification_first_sent_at",
        "members.verification_reminded_at",
        "oauth_codes.client_id",
        "oauth_codes.code_challenge",
        "oauth_codes.code_hash",
        "oauth_codes.expires_at",
        "oauth_codes.redirect_uri",
        "oauth_codes.session_hash",
        "oauth_tokens.client_id",
        "oauth_tokens.created_at",
        "oauth_tokens.expires_at",
        "oauth_tokens.session_hash",
        "oauth_tokens.token_hash",
        "password_reset_codes.account",
        "password_reset_codes.code_hash",
        "password_reset_codes.created_at",
        "password_reset_codes.email",
        "password_reset_codes.expires_at",
        "sessions.account",
        "sessions.created_at",
        "sessions.expires_at",
        "sessions.token_hash",
        "uploads.account",
        "uploads.content_type",
        "uploads.created_at",
        "uploads.id",
        "uploads.owner",
        "uploads.size",
        "verification_codes.account",
        "verification_codes.code_hash",
        "verification_codes.created_at",
        "verification_codes.email",
        "verification_codes.expires_at",
        "verification_codes.replaced_at",
    ], columns

with subtest("stored: a member's email stays with concierge, out of Ergo's database, and leaves the outbox once sent"):
    wait_for_mail(email)
    wait_for_empty_outbox()
    files = files_containing(email)
    assert files and all(file.startswith("/var/lib/private/concierge/concierge.db") for file in files), files

with subtest("not stored: failed sign-ins are counted in memory, and a restart forgets them"):
    for _ in range(5):
        assert graces_browser.sign_in("grace", "a-wrong-guess") == "401 "
    assert graces_browser.sign_in(**grace) == "429 "
    credenza.succeed("systemctl restart concierge")
    credenza.wait_for_open_port(8080)
    assert graces_browser.sign_in(**grace) == f"303 https://{members}/"

with subtest("email: news emails are off until a member turns them on, and they can turn them off"):
    assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "0\n"
    assert graces_browser.post("/settings", f"email={email}", "news=on").startswith("303 ")
    assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "1\n"
    assert graces_browser.post("/settings", f"email={email}").startswith("303 ")
    assert concierge_sql("SELECT news_opt_in FROM members WHERE account = 'grace'") == "0\n"

with subtest("leaving: an admin deletes the account, its email leaves the disk, and IRC stops taking it"):
    assert " 903 " in irc(**grace)
    concierge_admin("member", "delete", "grace")
    found = files_containing(email)
    assert found == [], found
    credenza.succeed("systemctl start concierge-vacuum")
    client.fail(irc_command(**grace))
    assert graces_browser.get("/", *outcome) == f"303 https://{members}/signin"

heidi: Account = {"account": "heidi", "password": "quarry-lantern-36"}
ivan: Account = {"account": "ivan", "password": "saffron-kettle-12"}
for member in [heidi, ivan]:
    register(**member)
    irc(**member)
said_in_channel = "the-canary-sings-in-the-lounge"
said_to_ivan = "the-canary-whispers-to-ivan"


def lounge_history():
    return irc("HISTORY #lounge 50", **heidi)


def restart_ergo(days_later: int = 0) -> None:
    credenza.succeed(f"date -s '+{days_later} days'")
    credenza.succeed("systemctl restart ergochat")
    credenza.wait_for_open_port(6697)


def wait_until_off_ergos_disk(*texts: str) -> None:
    credenza.wait_until_succeeds(
        f"systemctl start ergochat-vacuum && ! grep -rqaF {patterns(texts)} /var/lib/private/ergo", timeout=60
    )


with subtest("messages: channel and direct messages survive a restart"):
    irc(f"PRIVMSG #lounge :{said_in_channel}", f"PRIVMSG ivan :{said_to_ivan}", **heidi)
    restart_ergo()
    assert said_in_channel in lounge_history()
    assert said_to_ivan in irc("HISTORY heidi 50", **ivan)

with subtest("messages: a member can delete a message they sent, and it leaves the disk"):
    taken_back = "the-canary-takes-it-back"
    echoed = irc("CAP REQ :echo-message message-tags", f"PRIVMSG ivan :{taken_back}", **heidi)
    msgid = re.search(rf"[@;]msgid=([^ ;]+)\S* :heidi!\S+ PRIVMSG ivan :{taken_back}", echoed)
    assert msgid, echoed
    assert taken_back in irc("HISTORY heidi 50", **ivan)
    irc(f"REDACT ivan {msgid[1]}", **heidi)
    assert taken_back not in irc("HISTORY heidi 50", **ivan)
    wait_until_off_ergos_disk(taken_back)
    found = files_containing(taken_back)
    assert found == [], found

with subtest("messages: the page says how long they stay from the expire-time concierge is handed"):
    environment = credenza.succeed("systemctl show -P Environment concierge.service").split()
    assert "CONCIERGE_HISTORY_EXPIRE_TIME=2w" in environment, environment
    page = graces_browser.get("/privacy")
    assert "direct messages are kept on the server for 2 weeks," in page, page

# Moves the clock, so it goes last.
with subtest("messages: channel and direct messages stay two weeks, then leave the disk, and newer ones stay"):
    stored = [said_in_channel, said_to_ivan]
    assert all(files_containing(said) for said in stored)
    restart_ergo(13)
    assert said_in_channel in lounge_history()
    assert said_to_ivan in irc("HISTORY heidi 50", **ivan)
    restart_ergo(2)
    said_since = "the-canary-sings-two-weeks-later"
    irc(f"PRIVMSG #lounge :{said_since}", **heidi)
    wait_until_off_ergos_disk(*stored)
    found = files_containing(*stored)
    assert found == [], found
    history = lounge_history()
    assert said_in_channel not in history and said_since in history, history
    assert said_to_ivan not in irc("HISTORY heidi 50", **ivan)
    assert files_containing(said_since) != []
    owners = credenza.succeed("stat -c %u /var/lib/private/ergo /var/lib/private/ergo/ergo_history.db*").split()
    assert len(set(owners)) == 1, owners

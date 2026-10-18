import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, mail, subtest
    from helpers import (
        Browser,
        add_member,
        clear_mail,
        concierge_admin,
        concierge_sql,
        confirm_email,
        invite_code,
        link_in,
        mails_to,
        members,
        outbox,
        outbox_sql,
        sqlite_command,
        wait_for_empty_outbox,
        wait_for_mail,
    )

subject = "A test message from weblounge.org"
mailer_log = "journalctl -o cat -u concierge-mailer"
remind_log = "journalctl -o cat -u concierge-remind"
verification_subject = "Confirm your email for weblounge.org"
reminder_subject = "Reminder: confirm your email for weblounge.org"
verify_link = f"https://{members}/verify#"
key_query = sqlite_command(outbox, "SELECT public_key FROM mailer_key")


def queue_test_message(address: str) -> str:
    """Queues an admin's test message to address, and answers the id the mailer logs it by."""
    return concierge_admin("mail", "test", address).split()[-1]


def wait_for_log(line: str) -> None:
    credenza.wait_until_succeeds(f"{mailer_log} | grep -qxF {shlex.quote(line)}", timeout=30)


def outbox_size() -> int:
    return int(outbox_sql("SELECT count(*) FROM outbox"))


def remind() -> str:
    """Runs concierge remind as its timer would, and answers what it logged."""
    invocation = credenza.succeed("systemctl start concierge-remind && systemctl show -P InvocationID concierge-remind")
    credenza.succeed("journalctl --sync")
    return credenza.succeed(f"{remind_log} _SYSTEMD_INVOCATION_ID={invocation.strip()}")


def post_code(link: str) -> str:
    return Browser("scanner").post("/verify", f"code={invite_code(link)}", flags=["-w", "\n%{http_code}"])


def published_key() -> str:
    """The key the mailer published, once it has."""
    return credenza.wait_until_succeeds(f"{key_query} | grep '^age1'", timeout=30).strip()


credenza.wait_for_unit("concierge-mailer.service")
mail.wait_for_unit("mailpit-test.service")
mail.wait_for_open_port(465)

with subtest("the mailer publishes a new key each time it starts, for concierge to seal mail to"):
    first = published_key()
    credenza.succeed("systemctl restart concierge-mailer")
    credenza.wait_until_succeeds(f"{key_query} | grep '^age1' | grep -qvxF {first}", timeout=30)

with subtest("the mailer withdraws its key as it stops, and when it's killed before it starts again"):
    withdrew = "Withdrew the mailer's key; deleted 0 messages sealed to it, unsent"
    credenza.succeed("systemctl stop concierge-mailer")
    assert outbox_sql("SELECT count(*) FROM mailer_key").strip() == "0"
    credenza.succeed("systemctl start concierge-mailer")
    killed = published_key()
    credenza.succeed("systemctl kill --kill-whom=main -s KILL concierge-mailer")
    credenza.wait_until_succeeds(f"{key_query} | grep '^age1' | grep -qvxF {killed}", timeout=30)
    credenza.succeed("journalctl --sync")
    log = credenza.succeed(f"{mailer_log} | grep -E '^(Withdrew|Sending mail through)'").splitlines()
    assert log[-4:] == [withdrew, "Sending mail through mail:465", withdrew, "Sending mail through mail:465"], log

with subtest("an admin's test message reaches the mail server from noreply@, as plain text, and leaves the outbox"):
    queued = queue_test_message("alice@example.org")
    message = wait_for_mail("alice@example.org", subject)
    assert message["from"] == "weblounge.org <noreply@weblounge.org>", message
    assert message["to"] == ["alice@example.org"], message
    assert message["text"].startswith("An admin of weblounge.org sent this to check"), message
    wait_for_log(f"Sent message {queued}")
    assert outbox_size() == 0
    clear_mail()
    assert mails_to("alice@example.org") == []

with subtest("mail queued while the mail server is down waits, and goes out once it's back"):
    mail.succeed("systemctl stop mailpit-test")
    queued = queue_test_message("bob@example.org")
    failed = f"^Sending message {queued} failed .* on attempt 1$"
    credenza.wait_until_succeeds(f"{mailer_log} | grep -q {shlex.quote(failed)}", timeout=30)
    assert outbox_size() == 1
    mail.succeed("systemctl start mailpit-test")
    mail.wait_for_open_port(465)
    # The mailer tries again a minute after the first failure.
    wait_for_mail("bob@example.org", subject, timeout=120)
    wait_for_log(f"Sent message {queued}")
    assert outbox_size() == 0
    assert len(mails_to("bob@example.org")) == 1

with subtest("concierge remind runs hourly from a timer that catches up on missed runs"):
    credenza.succeed("systemctl is-active concierge-remind.timer")
    assert credenza.succeed("systemctl show -P Persistent concierge-remind.timer").strip() == "yes"
    assert "OnCalendar=*-*-* *:00:00 " in credenza.succeed("systemctl show -P TimersCalendar concierge-remind.timer")

with subtest("five days after the first link, concierge remind mails a fresh one, once"):
    add_member("grace", "willow-ember-36")
    first_link = link_in(wait_for_mail("grace@example.org", verification_subject), verify_link)
    # Only the times the reminder goes by move back, not the old link's
    # expiry, so it's still on file to say that a newer email replaced it.
    for table, column in [("members", "verification_first_sent_at"), ("verification_codes", "created_at")]:
        backdate = f"UPDATE {table} SET {column} = {column} - 5 * 24 * 60 * 60 WHERE account = 'grace'"
        concierge_sql(backdate, write=True)
    assert remind().strip() == "Queued 1 reminder"
    reminder = wait_for_mail("grace@example.org", reminder_subject)
    assert reminder["text"].splitlines()[0] == "Hi grace,", reminder
    assert "but you won't be able to invite others until" in reminder["text"], reminder
    fresh_link = link_in(reminder, verify_link)
    long_lines = [line for line in reminder["text"].splitlines() if len(line) > 76 and line != fresh_link]
    assert long_lines == [], long_lines
    replaced = post_code(first_link)
    assert replaced.endswith("\n422") and "A newer email was sent." in replaced, replaced
    confirmed = post_code(fresh_link)
    assert confirmed.endswith("\n200") and "Your email is confirmed." in confirmed, confirmed
    assert concierge_sql("SELECT email_verified_at IS NOT NULL FROM members WHERE account = 'grace'").strip() == "1"

with subtest("running concierge remind again mails nothing more"):
    assert remind().strip() == "Queued 0 reminders"
    wait_for_empty_outbox()
    assert [message["subject"] for message in mails_to("grace@example.org")] == [verification_subject, reminder_subject]

with subtest("member send-verification mails a link to whoever joined before links, none to the confirmed"):
    add_member("heidi", "saffron-quarry-52")
    add_member("ivan", "marble-thicket-17")
    wait_for_empty_outbox()
    clear_mail()
    concierge_sql("UPDATE members SET verification_first_sent_at = NULL WHERE account = 'heidi'", write=True)
    concierge_sql("DELETE FROM verification_codes WHERE account = 'heidi'", write=True)
    confirm_email("ivan")
    assert concierge_admin("member", "send-verification", "--unverified") == "sent heidi a link\n"
    link = link_in(wait_for_mail("heidi@example.org", verification_subject), verify_link)
    confirmed = post_code(link)
    assert confirmed.endswith("\n200") and "Your email is confirmed." in confirmed, confirmed
    assert concierge_admin("member", "send-verification", "ivan") == "ivan confirmed their email already\n"
    wait_for_empty_outbox()
    assert mails_to("ivan@example.org") == []

with subtest("the mailer's log names no recipient and no subject"):
    credenza.succeed("journalctl --sync")
    credenza.fail(f"{mailer_log} | grep -qF -e example.org -e {shlex.quote(subject)}")
    credenza.fail(f"{remind_log} | grep -qF example.org")

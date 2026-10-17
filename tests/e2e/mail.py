import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, mail, subtest
    from helpers import clear_mail, concierge_admin, mails_to, outbox_sql, wait_for_mail

subject = "A test message from weblounge.org"
mailer_log = "journalctl -o cat -u concierge-mailer"


def queue_test_message(address: str) -> str:
    """Queues an admin's test message to address, and answers the id the mailer logs it by."""
    return concierge_admin("mail", "test", address).split()[-1]


def wait_for_log(line: str) -> None:
    credenza.wait_until_succeeds(f"{mailer_log} | grep -qxF {shlex.quote(line)}", timeout=30)


def outbox_size() -> int:
    return int(outbox_sql("SELECT count(*) FROM outbox"))


credenza.wait_for_unit("concierge-mailer.service")
mail.wait_for_unit("mailpit-test.service")
mail.wait_for_open_port(465)

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

with subtest("the mailer's log names no recipient and no subject"):
    credenza.succeed("journalctl --sync")
    credenza.fail(f"{mailer_log} | grep -qF -e example.org -e {shlex.quote(subject)}")

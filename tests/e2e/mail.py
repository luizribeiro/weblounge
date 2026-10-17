import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, mail, subtest
    from helpers import concierge_admin, outbox_sql, wait_for_mail

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

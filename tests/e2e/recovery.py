# Members who forgot their password get a link to their confirmed email.
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import credenza, mail, subtest
    from helpers import (
        Account,
        Browser,
        add_member,
        clear_mail,
        confirm_email,
        mails_to,
        members,
        wait_for_empty_outbox,
        wait_for_mail,
    )

reset_subject = "Reset your password for weblounge.org"
olive: Account = {"account": "olive", "password": "juniper-harbor-63"}
pat: Account = {"account": "pat", "password": "cedar-lantern-58"}
stranger = Browser("stranger")


def forgot(who: str) -> str:
    """Asks for a reset link for who, and answers the page and its status."""
    return stranger.post("/forgot-password", f"who={who}", flags=["-w", "\n%{http_code}"])


credenza.wait_for_unit("concierge-mailer.service")
mail.wait_for_unit("mailpit-test.service")
mail.wait_for_open_port(465)
add_member(**olive)
add_member(**pat)
confirm_email("olive")
wait_for_empty_outbox()
clear_mail()

with subtest("the sign-in page leads to asking for a reset link"):
    signin = stranger.get("/signin", "-f")
    assert '<a href="/forgot-password">Forgot your password?</a>' in signin, signin
    page = stranger.get("/forgot-password", "-f")
    assert '<label for="who">Nick or email</label>' in page, page

with subtest("a confirmed member, an unconfirmed one and unknown names all get the same answer"):
    answers = {who: forgot(who) for who in ["olive", "pat", "pat@example.org", "nobody", "nobody@example.org"]}
    assert len(set(answers.values())) == 1, answers
    answer = answers["olive"]
    assert answer.endswith("\n200") and "<h1>Check your email.</h1>" in answer, answer

with subtest("only the confirmed member's address gets a link"):
    message = wait_for_mail("olive@example.org", reset_subject)
    assert message["text"].splitlines()[0] == "Hi olive,", message
    wait_for_empty_outbox()
    for address in ["pat@example.org", "nobody@example.org"]:
        assert mails_to(address) == [], address
    assert f"https://{members}/reset#" in message["text"], message

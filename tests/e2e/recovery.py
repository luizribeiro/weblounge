# Members who forgot their password get a link to their confirmed email.
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, mail, subtest
    from helpers import (
        Account,
        Browser,
        add_member,
        clear_mail,
        concierge_sql,
        confirm_email,
        invite_code,
        irc,
        irc_command,
        link_in,
        mails_to,
        members,
        outcome,
        wait_for_empty_outbox,
        wait_for_mail,
    )

reset_subject = "Reset your password for weblounge.org"
changed_subject = "Your password for weblounge.org was changed"
reset_link = f"https://{members}/reset#"
olive: Account = {"account": "olive", "password": "juniper-harbor-63"}
pat: Account = {"account": "pat", "password": "cedar-lantern-58"}
quinn: Account = {"account": "quinn", "password": "saffron-meadow-24"}
new_password = "copper-thistle-85"
stranger = Browser("stranger")


def forgot(who: str) -> str:
    """Asks for a reset link for who, and answers the page and its status."""
    return stranger.post("/forgot-password", f"who={who}", flags=["-w", "\n%{http_code}"])


def post_reset(link: str, password: str) -> str:
    """Posts the reset form with the code from link, as its button does."""
    fields = [f"code={invite_code(link)}", f"password={password}", f"confirmation={password}"]
    return Browser("scanner").post("/reset", *fields, flags=["-w", "\n%{http_code}"])


def reset_codes(account: str) -> str:
    return concierge_sql(f"SELECT count(*) FROM password_reset_codes WHERE account = '{account}'").strip()


credenza.wait_for_unit("concierge-mailer.service")
mail.wait_for_unit("mailpit-test.service")
mail.wait_for_open_port(465)
for member in [olive, pat, quinn]:
    add_member(**member)
confirm_email("olive")
confirm_email("quinn")
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
    link = link_in(message, reset_link)

with subtest("opening the link, as a mail scanner does, uses nothing"):
    for path in ["/reset", f"/reset?code={invite_code(link)}"]:
        page = stranger.get(path, "-f")
        assert '<h1 id="reset-title">Choose a new password.</h1>' in page, page
        assert "IRC apps that are connected now stay connected" in page, page
    assert reset_codes("olive") == "1"

with subtest("in a browser, the link sets a new password and sends the member to sign in"):
    olives = Browser("olive")
    assert olives.sign_in(**olive).startswith("303 ")
    result = client.succeed(f"recovery-browser reset {shlex.quote(link)} {new_password} 2>&1")
    assert "reset from the link" in result, result

with subtest("the old password stops working on IRC and the new one works, and sessions are ended"):
    client.fail(irc_command(**olive))
    assert " 903 " in irc(account="olive", password=new_password)
    assert olives.get("/", *outcome) == f"303 https://{members}/signin"
    assert olives.sign_in("olive", new_password) == f"303 https://{members}/"

with subtest("the member's confirmed address hears that the password changed, in plain mail"):
    notice = wait_for_mail("olive@example.org", changed_subject)
    assert notice["text"].splitlines()[0] == "Hi olive,", notice
    assert f"https://{members}/forgot-password" in notice["text"], notice

with subtest("a used link and an expired one fail cleanly and change nothing"):
    used = post_reset(link, "another-new-password-1")
    assert used.endswith("\n422") and "This link doesn&#39;t work any more." in used, used
    assert forgot("quinn").endswith("\n200")
    quinns = link_in(wait_for_mail("quinn@example.org", reset_subject), reset_link)
    concierge_sql("UPDATE password_reset_codes SET expires_at = unixepoch() WHERE account = 'quinn'", write=True)
    expired = post_reset(quinns, "another-new-password-2")
    assert expired.endswith("\n422") and "This link doesn&#39;t work any more." in expired, expired
    assert " 903 " in irc(**quinn)
    assert " 903 " in irc(account="olive", password=new_password)

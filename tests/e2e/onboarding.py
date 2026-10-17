import re
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, mail, subtest
    from helpers import (
        Account,
        Browser,
        clear_mail,
        concierge_admin,
        concierge_sql,
        invite_code,
        irc,
        irc_command,
        link_in,
        mails_to,
        members,
        outcome,
        wait_for_mail,
    )

erin: Account = {"account": "erin", "password": "lantern-orchard-71"}
frank: Account = {"account": "frank", "password": "copper-meadow-94"}
erins_browser = Browser("erin")
erins_email = "erin@example.org"
verification_subject = "Confirm your email for weblounge.org"


def email_verified_at(account: str) -> str:
    return concierge_sql(f"SELECT email_verified_at FROM members WHERE account = '{account}'").strip()


credenza.wait_for_unit("nginx.service")
credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(8089)
mail.wait_for_unit("mailpit-test.service")

with subtest("an admin mints a root invite on join.weblounge.org and the invitee opens it over HTTPS"):
    link = concierge_admin("invite", "create", "--root").strip()
    assert re.fullmatch(r"https://join\.weblounge\.org/#[A-Za-z0-9_-]{22}", link), link
    opened = erins_browser.get("/invite", "-f")
    assert "<h1 data-invite-heading>You're invited.</h1>" in opened, opened
    assert 'name="code"' in opened and 'autocomplete="new-password"' in opened, opened

with subtest("signing up answers with the welcome page, uncached, and signs the new member in"):
    response = erins_browser.sign_up(link, **erin, flags=["-i"])
    headers, page = response.split("\r\n\r\n", 1)
    assert " 200" in headers.splitlines()[0], headers
    assert "cache-control: no-store" in headers.lower(), headers
    assert "<h1>You're in, erin.</h1>" in page, page
    assert '<a class="button button-primary" href="https://irc.weblounge.org/">Open chat</a>' in page, page
    assert "<code>irc.weblounge.org</code>" in page and "<code>6697</code>" in page, page
    assert erin["password"] not in page
    assert f"We sent a link to <strong>{erins_email}</strong> to confirm your email." in page, page
    home = erins_browser.get("/", "-f")
    assert "Pull up a chair, erin." in home, home

with subtest("the used invite's code stops working"):
    refused = Browser("latecomer").sign_up(link, "latecomer", "pewter-harbor-58", flags=["-w", "\n%{http_code}"])
    assert refused.endswith("\n422") and "This invite doesn&#39;t work." in refused, refused

with subtest("the new member signs in to the website with the password they chose"):
    assert erins_browser.post("/signout").startswith("303 ")
    assert erins_browser.sign_in(**erin) == f"303 https://{members}/"

with subtest("the new member logs in to IRC with SASL and lands in #lounge"):
    session = irc(**erin)
    assert " 903 " in session, session
    assert re.search(r"^:erin!\S+ JOIN #lounge", session, re.M), session

with subtest("until the new member confirms her email, she's refused an invite, and the Invites tab says why"):
    refused = erins_browser.post("/invites")
    assert refused == f"303 https://{members}/invites", refused
    tab = erins_browser.get("/invites", "-f")
    assert "You can't invite others until you confirm your email." in tab, tab
    assert concierge_sql("SELECT count(*) FROM invites WHERE created_by = 'erin'").strip() == "0"

with subtest("in a browser, the pages remind her to confirm by her date, and the Invites tab leads to the Account tab"):
    reminded = client.succeed(f"verify-browser unconfirmed erin {shlex.quote(erin['password'])} 2>&1")
    assert "lead to the Account tab" in reminded, reminded

with subtest("signing up mails a link to confirm the email, in short plain lines, under a subject without the nick"):
    message = wait_for_mail(erins_email, verification_subject)
    assert message["from"] == "weblounge.org <noreply@weblounge.org>", message
    assert "erin" not in message["subject"].lower(), message
    assert message["text"].splitlines()[0] == "Hi erin,", message
    first_link = link_in(message, f"https://{members}/verify#")
    long_lines = [line for line in message["text"].splitlines() if len(line) > 76 and line != first_link]
    assert long_lines == [], long_lines

with subtest("asking for another link at once is refused, without mailing"):
    refused = erins_browser.post("/verification-link", flags=["-w", "\n%{http_code}"])
    assert refused.endswith("\n429") and "A link went out a few minutes ago." in refused, refused
    assert len(mails_to(erins_email)) == 1

with subtest("from the Account tab, a new link replaces the one signup mailed"):
    # The limits live in memory, so a restart lets the test ask again now.
    credenza.succeed("systemctl restart concierge")
    credenza.wait_for_open_port(8080)
    clear_mail()
    resent = client.succeed(f"verify-browser resend erin {shlex.quote(erin['password'])} 2>&1")
    assert "asked for a new link" in resent, resent
    verify_link = link_in(wait_for_mail(erins_email, verification_subject), f"https://{members}/verify#")
    assert verify_link != first_link
    replaced = Browser("scanner").post("/verify", f"code={invite_code(first_link)}", flags=["-w", "\n%{http_code}"])
    assert replaced.endswith("\n422") and "A newer email was sent." in replaced, replaced

with subtest("opening a verification link, in curl or a browser, verifies nothing until Confirm is pressed"):
    verify_code = invite_code(verify_link)
    assert "Confirm my email" in erins_browser.get(f"/verify?code={verify_code}", "-f")
    looked = client.succeed(f"verify-browser look {shlex.quote(verify_link)} 2>&1")
    assert "verifies nothing by loading" in looked, looked
    assert email_verified_at("erin") == ""

with subtest("in a browser, pressing Confirm verifies the new member's email, and the link works once"):
    confirmed = client.succeed(f"verify-browser confirm {shlex.quote(verify_link)} 2>&1")
    assert "confirmed from the link" in confirmed, confirmed
    assert email_verified_at("erin") != ""
    again = Browser("scanner").post("/verify", f"code={verify_code}", flags=["-w", "\n%{http_code}"])
    assert again.endswith("\n422") and "This link doesn&#39;t work any more." in again, again

with subtest("in a browser, the new member's invite link names them and signs someone up under them"):
    _, franks_link = erins_browser.create_invite()
    args = shlex.join([franks_link, "erin", link, frank["account"], frank["password"]])
    signed_up = client.succeed(f"invite-browser {args} 2>&1")
    assert "signed up from the link" in signed_up, signed_up
    tab = erins_browser.get("/invites", "-f")
    assert "<li>frank</li>" in tab, tab
    assert "frank" not in concierge_admin("member", "reconcile")
    assert " 903 " in irc(**frank)
    _, open_link = erins_browser.create_invite()
    client.copy_from_machine("/tmp/screens")

with subtest("member reset-password locks the old password out of IRC and ends the website session"):
    new_password = concierge_admin("member", "reset-password", "erin").strip()
    client.fail(irc_command(**erin))
    assert erins_browser.get("/", *outcome) == f"303 https://{members}/signin"
    assert " 903 " in irc(account="erin", password=new_password)

with subtest("the passwords and link codes never reach the journal, nor nginx's log the verification code"):
    credenza.succeed("journalctl --sync")
    codes = [invite_code(link), invite_code(franks_link), invite_code(first_link), verify_code]
    for secret in [erin["password"], frank["password"], new_password, *codes]:
        credenza.fail(f"journalctl | grep -qF {secret}")
    for code in [invite_code(first_link), verify_code]:
        credenza.fail(f"grep -qF {code} /var/log/nginx/access.log")

with subtest("concierge's database keeps no invite code, only its hash"):
    for code in [invite_code(link), invite_code(franks_link), invite_code(open_link)]:
        credenza.fail(f"grep -aqF {code} /var/lib/private/concierge/concierge.db*")

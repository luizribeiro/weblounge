import hashlib
import re
import secrets
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        Browser,
        concierge_admin,
        concierge_sql,
        invite_code,
        irc,
        irc_command,
        members,
        outcome,
    )

erin: Account = {"account": "erin", "password": "lantern-orchard-71"}
frank: Account = {"account": "frank", "password": "copper-meadow-94"}
erins_browser = Browser("erin")


def email_verified_at(account: str) -> str:
    return concierge_sql(f"SELECT email_verified_at FROM members WHERE account = '{account}'").strip()


credenza.wait_for_unit("nginx.service")
credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(8089)

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

with subtest("opening a verification link, in curl or a browser, verifies nothing until Confirm is pressed"):
    verify_code = secrets.token_urlsafe(32)
    code_hash = hashlib.sha256(verify_code.encode()).hexdigest()
    concierge_sql(
        "INSERT INTO verification_codes (code_hash, account, email, created_at, expires_at) "
        f"SELECT x'{code_hash}', account, email, unixepoch(), unixepoch() + 172800 FROM members WHERE account = 'erin'",
        write=True,
    )
    verify_link = f"https://{members}/verify#{verify_code}"
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
    client.copy_from_machine("/tmp/screens")

with subtest("member reset-password locks the old password out of IRC and ends the website session"):
    new_password = concierge_admin("member", "reset-password", "erin").strip()
    client.fail(irc_command(**erin))
    assert erins_browser.get("/", *outcome) == f"303 https://{members}/signin"
    assert " 903 " in irc(account="erin", password=new_password)

with subtest("the passwords and link codes never reach the journal, nor nginx's log the verification code"):
    credenza.succeed("journalctl --sync")
    codes = [invite_code(link), invite_code(franks_link), verify_code]
    for secret in [erin["password"], frank["password"], new_password, *codes]:
        credenza.fail(f"journalctl | grep -qF {secret}")
    credenza.fail(f"grep -qF {verify_code} /var/log/nginx/access.log")

with subtest("concierge's database keeps no invite code, only its hash"):
    for code in [invite_code(link), invite_code(franks_link), invite_code(open_link)]:
        credenza.fail(f"grep -aqF {code} /var/lib/private/concierge/concierge.db*")

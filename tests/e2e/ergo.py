import json
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import Account, concierge_admin, ergo_api, irc, irc_command, register

alice: Account = {"account": "alice", "password": "correct-horse"}
bob: Account = {"account": "bob", "password": "battery-staple"}
carol: Account = {"account": "carol", "password": "tr0ub4dor-and-3"}

with subtest("ergo serves IRC over TLS on 6697 with irc.weblounge.org's certificate"):
    credenza.wait_for_unit("ergochat.service")
    credenza.wait_for_open_port(6697)
    names = client.succeed(
        "openssl s_client -connect credenza:6697 -servername irc.weblounge.org </dev/null 2>/dev/null"
        " | openssl x509 -noout -ext subjectAltName"
    )
    assert "DNS:irc.weblounge.org" in names, names

with subtest("a renewed certificate reaches ergo on reload, without a restart"):
    fingerprint = "openssl s_client -connect credenza:6697 </dev/null 2>/dev/null | openssl x509 -noout -fingerprint"
    before = client.succeed(fingerprint)
    pid = credenza.succeed("systemctl show -P MainPID ergochat")
    credenza.succeed("systemctl restart acme-irc.weblounge.org.service")
    credenza.succeed("systemctl try-reload-or-restart ergochat")
    client.wait_until_succeeds(f"{fingerprint} | grep -vqF '{before.strip()}'", timeout=10)
    assert credenza.succeed("systemctl show -P MainPID ergochat") == pid

with subtest("ergo's API answers the generated token"):
    body, status = ergo_api("status")
    assert status == "200" and '"success":true' in body, (status, body)

with subtest("the secrets in RAM outlive the secrets service stopping, as a deploy may, and concierge reaches ergo"):
    in_ram = "cat /run/weblounge-secrets/ergo-api-token /run/weblounge-secrets/oauth-introspection-secret"
    before = credenza.succeed(in_ram)
    credenza.succeed("systemctl stop weblounge-secrets")
    credenza.succeed("systemctl start ergochat concierge")
    assert credenza.succeed(in_ram) == before
    credenza.wait_for_open_port(8089)
    assert "ergo: http://127.0.0.1:8089/ answers" in concierge_admin("status")

with subtest("an account created through the API logs in over TLS with SASL"):
    body = register(**alice)
    assert '"success":true' in body, body
    session = irc(**alice)
    assert " 903 " in session and " 001 alice " in session, session

with subtest("ergo hashes passwords with bcrypt at cost 12"):
    # base64 of "$2a$12$", the start of the stored hash.
    credenza.wait_until_succeeds('grep -q \'"PassphraseHash":"JDJhJDEyJ\' /var/lib/ergo/ircd.db', timeout=10)

with subtest("logging in shows weblounge's MOTD"):
    assert ":- Pull up a chair and make yourself at home." in session, session

with subtest("members' addresses are cloaked under weblounge"):
    whois = irc("WHOIS alice", **alice)
    assert re.search(r" 311 alice alice \S+ \S+\.weblounge ", whois), whois

with subtest("clients on credenza reach ergo by its public name over loopback"):
    assert credenza.succeed("getent hosts irc.weblounge.org").split()[0] == "127.0.0.1"
    session = credenza.succeed(irc_command(host="irc.weblounge.org", **alice))
    assert " 001 alice " in session, session

with subtest("logging in joins #lounge"):
    assert re.search(r"^:alice!\S+ JOIN #lounge", session, re.M), session

with subtest("members stay in their channels, away, after disconnecting"):
    register(**bob)
    seen = irc("NAMES #lounge", "WHOIS alice", **bob)
    assert re.search(r" 353 bob = #lounge :.*\balice\b", seen), seen
    assert " 301 bob alice " in seen, seen

with subtest("members may delete their own messages from history"):
    reply = irc("PRIVMSG HistServ :DELETE #lounge no-such-message", **alice)
    assert "NOTICE alice :Message not found" in reply, reply

with subtest("ergo advertises web push for mobile clients"):
    assert "VAPID=" in session, session

with subtest("the API changes a member's password, and only the new one logs in"):
    register(**carol)
    renewed: Account = {**carol, "password": "a-longer-generated-one"}
    body, _ = ergo_api("ns/passwd", accountName=carol["account"], passphrase=renewed["password"])
    assert json.loads(body)["success"], body
    client.fail(irc_command(**carol))
    renewed_session = irc(**renewed)
    assert " 903 " in renewed_session, renewed_session

with subtest("the API reads and changes a member's always-on setting"):
    body, _ = ergo_api("ns/saget", accountName=carol["account"])
    assert json.loads(body)["alwaysOn"] == "default", body
    body, _ = ergo_api("ns/saset", accountName=carol["account"], alwaysOn="off")
    assert json.loads(body)["alwaysOn"] == "off", body
    body, _ = ergo_api("ns/saget", accountName=carol["account"])
    assert json.loads(body)["alwaysOn"] == "off", body

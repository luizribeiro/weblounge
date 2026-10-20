import hashlib
import hmac
import json
import re
import secrets
import shlex
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from driver import client, credenza, subtest
    from helpers import (
        Account,
        Browser,
        access_token,
        add_member,
        authorize,
        concierge_admin,
        concierge_sql,
        ergo_api,
        exchange,
        files_host,
        fixtures,
        get_file,
        https,
        irc,
        irc_command,
        members,
        not_found_page,
        pkce,
    )

grace: Account = {"account": "grace", "password": "a-kettle-on-the-hob"}
ada: Account = {"account": "ada", "password": "the-analytical-engine"}
graces_browser = Browser("grace")
gamja_connects = f"connect-src 'self' wss://irc.weblounge.org/socket https://{members} https://{files_host}/upload;"


credenza.wait_for_unit("nginx.service")
credenza.wait_for_unit("concierge.service")
credenza.wait_for_open_port(8080)
credenza.wait_for_unit("ergochat.service")
credenza.wait_for_open_port(8089)

add_member(**grace)
assert graces_browser.sign_in(**grace) == f"303 https://{members}/"

with subtest("a signed-in member trades a PKCE authorization code for a token"):
    verifier, challenge = pkce()
    code = authorize(graces_browser, challenge)
    status, answer = exchange(code, verifier)
    assert status == "200" and answer["token_type"] == "Bearer", (status, answer)
    token = answer["access_token"]

with subtest("the code is single-use and bound to its verifier"):
    status, answer = exchange(code, verifier)
    assert (status, answer) == ("400", {"error": "invalid_grant"}), (status, answer)
    other_verifier, _ = pkce()
    status, _ = exchange(authorize(graces_browser, challenge), other_verifier)
    assert status == "400", status

with subtest("the token logs in to IRC with SASL OAUTHBEARER and lands in #lounge"):
    session = irc(token=token)
    assert " 903 " in session, session
    assert re.search(r"^:grace!\S+ JOIN #lounge", session, re.M), session

with subtest("irc.weblounge.org serves gamja, set up to sign members in through concierge"):
    page = https("irc.weblounge.org", "/", "-f")
    assert "<title>gamja IRC client</title>" in page, page
    config = json.loads(https("irc.weblounge.org", "/config.json", "-f"))
    assert config == {
        "server": {
            "url": "wss://irc.weblounge.org/socket",
            "autojoin": "#lounge",
            "auth": "oauth2",
            "autoconnect": True,
        },
        "oauth2": {"url": f"https://{members}", "client_id": "gamja"},
    }, config

with subtest("gamja wears the network's favicon"):
    assert '<link rel="icon" type="image/svg+xml" href="favicon.svg">' in page, page
    icon = https("irc.weblounge.org", "/favicon.svg", "-f", "-w", "\n%{content_type}")
    svg, content_type = icon.rsplit("\n", 1)
    assert content_type == "image/svg+xml" and 'id="mark"' in svg, icon

with subtest("unknown paths on irc.weblounge.org get the not-found page, gamja's CSP and all"):
    not_found_page("irc.weblounge.org", "/no-such-page")
    headers = https("irc.weblounge.org", "/no-such-page", "-I").lower()
    assert gamja_connects in headers, headers

with subtest("gamja's CSP lets it reach Ergo's WebSocket, concierge and the upload endpoint, and nothing else"):
    headers = https("irc.weblounge.org", "/", "-fI").lower()
    [policy] = re.findall(r"^content-security-policy: (.*)$", headers, re.M)
    assert gamja_connects in policy, policy
    assert "default-src 'none'" in policy and "frame-ancestors 'none'" in policy, policy

with subtest("the token logs in over gamja's WebSocket too"):
    session = irc(token=token, websocket="irc.weblounge.org")
    assert " 903 " in session, session
    assert re.search(r"^:grace!\S+ JOIN #lounge", session, re.M), session

with subtest("Ergo refuses WebSockets from pages on other origins"):
    upgrade = [
        "--http1.1",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "-H",
        "Connection: Upgrade",
        "-H",
        "Upgrade: websocket",
        "-H",
        "Sec-WebSocket-Version: 13",
        "-H",
        "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
    ]
    status = https("irc.weblounge.org", "/socket", *upgrade, "-H", "Origin: https://evil.example")
    assert status == "403", status

with subtest("a garbage token is refused"):
    reply = client.fail(irc_command(token="q7Hx2mZp9vLcT4wRfBa-_wq7Hx2mZp9vLcT4wRfBa-_") + " 2>&1")
    assert "SASL OAUTHBEARER login failed" in reply, reply

with subtest("signing out ends the token"):
    revoked = access_token(graces_browser)
    assert " 903 " in irc(token=revoked)
    assert graces_browser.post("/signout").startswith("303 ")
    client.fail(irc_command(token=revoked))
    assert graces_browser.sign_in(**grace).startswith("303 ")

with subtest("a token for a member without an Ergo account creates none"):
    # A member row whose Ergo account is gone, signed in with a session
    # forged from the machine's session key.
    key = credenza.succeed("cat /var/lib/weblounge-secrets/session-key").strip()
    session = secrets.token_urlsafe(32)[:43]
    session_hash = hmac.new(key.encode(), session.encode(), hashlib.sha256).hexdigest()
    concierge_sql(
        "INSERT INTO members (account, email, joined_at) VALUES ('ghost', 'ghost@example.org', 0);"
        f"INSERT INTO sessions VALUES (x'{session_hash}', 'ghost', 0, 4000000000);",
        write=True,
    )
    ghosts_browser = Browser("ghost")
    client.succeed(
        f"printf 'members.weblounge.org\\tFALSE\\t/\\tTRUE\\t0\\t__Host-session\\t{session}\\n' > /tmp/cookies-ghost"
    )
    ghost_token = access_token(ghosts_browser)
    client.fail(irc_command(token=ghost_token))
    body, _ = ergo_api("ns/info", accountName="ghost")
    assert body.strip() == '{"success":false}', body

with subtest("in a browser, a signed-in member opens irc.weblounge.org and lands in #lounge"):
    page = client.succeed(f"webchat-browser {grace['account']} {shlex.quote(grace['password'])} 2>&1")
    assert "#lounge" in page, page

with subtest("in a browser, pasting an image or picking one with gamja's upload button puts its link in the composer"):
    paste = ["webchat-upload-browser", grace["account"], grace["password"], f"{fixtures}/photo.png"]
    links = json.loads(client.succeed(shlex.join(paste)))
    ways = {f"{width} {how}" for width in ["phone", "desktop"] for how in ["paste", "button"]}
    assert set(links) == ways and len(set(links.values())) == len(ways), links
    for way, link in links.items():
        assert re.fullmatch(rf"https://{files_host}/[A-Za-z0-9_-]{{22}}\.png", link), (way, link)
        assert get_file(link)["status"] == 200, link

with subtest("gamja shows each line once when one arrives while it catches up after a reconnect"):
    add_member(**ada)
    browser = ["webchat-reconnect-browser", grace["account"], grace["password"], ada["account"], ada["password"]]
    shown = json.loads(client.succeed(shlex.join(browser)))
    assert set(shown.values()) == {1}, shown

with subtest("a reboot makes new secrets in RAM, and ergo, concierge, the web client's login and bender take them"):
    add_member("luiz", "plain-old-vanilla")
    credenza.succeed("systemctl restart bender-account.service weblounge-bot-bender.service")
    in_ram = [
        f"/run/weblounge-secrets/{name}" for name in ["ergo-api-token", "oauth-introspection-secret", "bender-password"]
    ]
    before = credenza.succeed(shlex.join(["cat", *in_ram])).split()
    credenza.shutdown()
    credenza.start()
    credenza.wait_for_unit("weblounge-bot-bender.service")
    credenza.wait_for_open_port(8080)
    credenza.wait_for_open_port(8089)
    after = credenza.succeed(shlex.join(["cat", *in_ram])).split()
    assert len(after) == len(before) and not set(before) & set(after), (before, after)
    assert "ergo: http://127.0.0.1:8089/ answers" in concierge_admin("status")
    rebooted = access_token(graces_browser)
    names = (
        irc_command("JOIN #bots", "NAMES #bots", token=rebooted) + " | grep ' 353 grace = #bots :.*bender' >/dev/null"
    )
    client.wait_until_succeeds(names, timeout=60)

with subtest("tokens and codes never reach the journal"):
    credenza.succeed("journalctl --sync")
    for secret in [token, revoked, ghost_token, rebooted, code, verifier]:
        credenza.fail(f"journalctl | grep -qF -- {secret}")

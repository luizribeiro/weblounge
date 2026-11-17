import base64
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
        WebClient,
        access_token,
        add_member,
        authorize,
        authorize_path,
        beta_client,
        concierge_admin,
        concierge_sql,
        ergo_api,
        exchange,
        files_host,
        https,
        irc,
        irc_command,
        members,
        not_found_page,
        pkce,
        push,
        session_token,
    )

grace: Account = {"account": "grace", "password": "a-kettle-on-the-hob"}
graces_browser = Browser("grace")
irc_host = "irc.weblounge.org"
beta = "beta.weblounge.org"
channels = {irc_host: "release", beta: "beta"}


def connects(host: str) -> str:
    return f"connect-src 'self' wss://{host}/socket https://{members} https://{files_host}/upload https://{push};"


def client_headers(host: str, path: str) -> str:
    return https(host, path, "-fI").lower()


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

with subtest("the token logs in over irc.weblounge.org's WebSocket too"):
    session = irc(token=token, websocket=irc_host)
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

pages = {}
for host, channel in channels.items():
    with subtest(f"{host} serves the web client's {channel} channel, set up to sign members in through concierge"):
        page = pages[host] = https(host, "/", "-f")
        assert "<title>weblounge</title>" in page, page
        body, content_type = https(host, "/config.json", "-f", "-w", "\n%{content_type}").rsplit("\n", 1)
        assert content_type == "application/json", content_type
        config = json.loads(body)
        assert config == {
            "mode": "hosted",
            "channel": channel,
            "network": {"name": "weblounge", "endpoint": f"wss://{host}/socket", "autojoin": ["#lounge"]},
            "oauth": {
                "issuer": f"https://{members}",
                "client_id": "weblounge-client",
                "redirect_uri": f"https://{host}/",
                "session_token_endpoint": f"https://{members}/oauth/session-token",
            },
            "push": {"relay": f"https://{push}", "relay_vapid": config["push"]["relay_vapid"]},
        }, config
        vapid_key = base64.urlsafe_b64decode(config["push"]["relay_vapid"] + "==")
        assert len(vapid_key) == 65 and vapid_key[0] == 4, "relay_vapid is not an uncompressed P-256 point"

pins = json.loads(client.succeed("cat /etc/client-pins.json"))
for host, channel in channels.items():
    with subtest(f"{host}'s version.json names the {channel} pin's commit and version"):
        body, content_type = https(host, "/version.json", "-f", "-w", "\n%{content_type}").rsplit("\n", 1)
        assert content_type == "application/json", content_type
        version = json.loads(body)
        assert version == {**pins[channel], "channel": channel}, (version, pins)
        assert re.fullmatch(r"[0-9a-f]{40}", version["commit"]), version


def client_asset(referrer: str, pattern: str) -> str:
    [path] = set(re.findall(rf"/?(?:assets/)?({pattern})", referrer))
    return f"/assets/{path}"


for host in channels:
    with subtest(f"{host}'s hashed assets keep a year, and the rest revalidates"):
        page = pages[host]
        script = client_asset(page, r"index-[\w-]+\.js")
        worker = client_asset(https(host, script, "-f"), r"engine\.shared-worker-[\w-]+\.js")
        wasm = client_asset(https(host, worker, "-f"), r"engine_bg-[\w-]+\.wasm")
        for path in [script, worker, wasm]:
            headers = client_headers(host, path)
            assert "cache-control: public, max-age=31536000, immutable" in headers, (path, headers)
            assert connects(host) in headers, (path, headers)
        assert "content-type: application/wasm" in client_headers(host, wasm)
        assert "content-type: text/javascript" in client_headers(host, "/service-worker.js")
        startup = re.findall(r'href="(/startup/[^"]+\.png)"', page)[0]
        for path in [
            "/",
            "/config.json",
            "/version.json",
            "/manifest.webmanifest",
            "/icons/favicon.svg",
            "/service-worker.js",
            startup,
        ]:
            headers = client_headers(host, path)
            assert "cache-control: no-cache" in headers, (path, headers)
            assert connects(host) in headers, (path, headers)

    with subtest(f"a reload of {host}'s deep links gets the app, while a missing asset is a 404"):
        deep = https(host, "/weblounge/lounge", "-fi")
        assert "<title>weblounge</title>" in deep, deep
        assert "cache-control: no-cache" in deep.lower(), deep
        not_found_page(host, "/assets/no-such-chunk.js")

    with subtest(
        f"{host}'s CSP lets the client run wasm in a worker, reach Ergo, concierge and uploads, and show them"
    ):
        [policy] = re.findall(r"^content-security-policy: (.*)$", client_headers(host, "/"), re.M)
        assert connects(host) in policy, policy
        for directive in [
            "script-src 'self' 'wasm-unsafe-eval'",
            "worker-src 'self'",
            "style-src 'self';",
            f"img-src 'self' blob: https://{files_host};",
            "font-src 'self'",
            "manifest-src 'self'",
            "default-src 'none'",
            "frame-ancestors 'none'",
        ]:
            assert directive in policy, (directive, policy)

    with subtest(f"{host}'s manifest is served as one, so phones can install it"):
        manifest = client_headers(host, "/manifest.webmanifest")
        assert "content-type: application/manifest+json" in manifest, manifest

with subtest("concierge signs members in to the beta client, whose token logs in over beta's WebSocket"):
    metadata = https(members, "/.well-known/oauth-authorization-server", "-fi", "-H", f"Origin: https://{beta}")
    head, body = metadata.split("\r\n\r\n", 1)
    assert f"access-control-allow-origin: https://{beta}" in head.lower(), head
    endpoints = json.loads(body)
    assert endpoints["issuer"] == f"https://{members}", endpoints
    for endpoint in ["authorization_endpoint", "token_endpoint"]:
        assert endpoints[endpoint].startswith(f"{endpoints['issuer']}/"), endpoints
    verifier, challenge = pkce()
    code = authorize(graces_browser, challenge, client=beta_client)
    status, answer = exchange(code, verifier, client=beta_client)
    assert status == "200" and answer["token_type"] == "Bearer", (status, answer)
    session = irc(token=answer["access_token"], websocket=beta, nick="*")
    assert " 903 " in session, session
    assert re.search(r"^:grace!\S+ JOIN #lounge", session, re.M), session

with subtest("a code issued at irc.weblounge.org buys nothing at beta's"):
    verifier, challenge = pkce()
    status, answer = exchange(authorize(graces_browser, challenge), verifier, client=beta_client)
    assert (status, answer) == ("400", {"error": "invalid_grant"}), (status, answer)

with subtest("gamja's client id gets a sign-in error page, not a redirect or a code"):
    gamja: WebClient = {"id": "gamja", "origin": f"https://{irc_host}"}
    refused = graces_browser.get(authorize_path(pkce()[1], client=gamja), "-i")
    assert refused.startswith("HTTP/2 400"), refused
    assert "location:" not in refused.lower(), refused
    assert "This sign-in link isn&#39;t one of ours." in refused, refused

with subtest("the beta client's page gets a token for the member's session in the background, which logs in"):
    answer = session_token(graces_browser)
    assert answer["status"] == 200, answer
    headers = answer["headers"]
    assert headers["access-control-allow-origin"] == f"https://{beta}", answer
    assert headers["access-control-allow-credentials"] == "true", answer
    assert headers["cache-control"] == "no-store", answer
    granted = json.loads(answer["body"])
    assert granted["token_type"] == "Bearer" and granted["expires_in"] == 15 * 60, granted
    background = granted["access_token"]
    session = irc(token=background, websocket=beta, nick="*")
    assert " 903 " in session, session
    assert re.search(r"^:grace!\S+ JOIN #lounge", session, re.M), session

with subtest("without a session the beta client's page is told to sign in, and other sites get no token"):
    signed_out = session_token()
    assert signed_out["status"] == 401, signed_out
    assert json.loads(signed_out["body"]) == {"error": "login_required"}, signed_out
    evil = session_token(graces_browser, origin="https://evil.example")
    assert evil["status"] == 403, evil
    assert "access-control-allow-origin" not in evil["headers"], evil

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

for host in channels:
    with subtest(f"in WebKit, a signed-out member signs in where {host} sends them, and lands in #lounge"):
        signin = ["client-signin-browser", f"https://{host}", grace["account"], grace["password"]]
        report = json.loads(client.succeed(shlex.join(signin)))
        signed_out, signed_in = report["signed out"], report["signed in"]
        sign_in_page, callback, cleaned = signed_out["visited"][1:]
        assert sign_in_page.startswith(f"https://{members}/signin?next=%2Foauth%2Fauthorize%3F"), report
        assert re.fullmatch(rf"https://{host}/\?code=[\w-]+&state=[\w-]+", callback), report
        assert cleaned == signed_out["url"] == signed_in["url"] == f"https://{host}/", report
        assert signed_out["lounge"] and signed_in["lounge"], report

    with subtest(f"in WebKit, {host}'s client of a member signed in to concierge joins without leaving {host}"):
        fresh = report["members cookies only"]
        assert fresh["lounge"], report
        assert all(url.startswith(f"https://{host}/") for url in fresh["visited"]), report

with subtest("a reboot makes new secrets in RAM, which ergo, concierge and the web login take, but keeps bender's"):
    in_ram = [f"/run/weblounge-secrets/{name}" for name in ["ergo-api-token", "oauth-introspection-secret"]]
    before = credenza.succeed(shlex.join(["cat", *in_ram])).split()
    benders = credenza.succeed("cat /run/secrets/bender-password")
    credenza.shutdown()
    credenza.start()
    credenza.wait_for_unit("louro.service")
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
    assert credenza.succeed("cat /run/secrets/bender-password") == benders

with subtest("tokens and codes never reach the journal"):
    credenza.succeed("journalctl --sync")
    for secret in [token, background, revoked, ghost_token, rebooted, code, verifier]:
        credenza.fail(f"journalctl | grep -qF -- {secret}")

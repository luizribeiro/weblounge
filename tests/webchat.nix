{ pkgs, vmTest, ... }:

vmTest {
  browsers.webchat-browser = ./webchat-browser.py;

  script = ''
    import base64
    import hashlib
    import hmac
    import re
    import secrets
    from urllib.parse import parse_qs, urlencode, urlsplit

    webchat = "https://irc.weblounge.org"
    grace = {"account": "grace", "password": "a-kettle-on-the-hob"}
    graces_browser = Browser("grace")

    def authorize(browser, challenge, state="a-random-state"):
        """What gamja does on load: answers the code concierge sends back."""
        query = urlencode({"response_type": "code", "client_id": "gamja", "redirect_uri": f"{webchat}/",
                           "state": state, "code_challenge": challenge, "code_challenge_method": "S256"})
        redirect = browser.get(f"/oauth/authorize?{query}", *outcome).split(" ", 1)[1]
        assert redirect.startswith(f"{webchat}/?"), redirect
        params = parse_qs(urlsplit(redirect).query)
        assert params["state"] == [state], redirect
        return params["code"][0]

    def exchange(code, verifier, origin=webchat):
        fields = {"grant_type": "authorization_code", "code": code, "redirect_uri": f"{webchat}/",
                  "client_id": "gamja", "code_verifier": verifier}
        flags = [arg for field in fields.items() for arg in ["--data-urlencode", "=".join(field)]]
        response = https(members, "/oauth/token", "-H", f"Origin: {origin}", "-w", "\n%{http_code}", *flags)
        body, status = response.rsplit("\n", 1)
        return status, json.loads(body)

    def pkce():
        verifier = secrets.token_urlsafe(32)
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        return verifier, challenge

    def access_token(browser):
        verifier, challenge = pkce()
        status, answer = exchange(authorize(browser, challenge), verifier)
        assert status == "200", (status, answer)
        return answer["access_token"]

    credenza.wait_for_unit("nginx.service")
    credenza.wait_for_unit("concierge.service")
    credenza.wait_for_open_port(8080)
    credenza.wait_for_unit("ergochat.service")
    credenza.wait_for_open_port(8089)

    register(**grace)
    concierge_admin("member", "adopt", "grace")
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
            "server": {"url": "wss://irc.weblounge.org/socket", "autojoin": "#lounge", "auth": "oauth2", "autoconnect": True},
            "oauth2": {"url": f"https://{members}", "client_id": "gamja"},
        }, config

    with subtest("gamja's CSP lets it reach Ergo's WebSocket and concierge, and nothing else"):
        headers = https("irc.weblounge.org", "/", "-fI").lower()
        [policy] = re.findall(r"^content-security-policy: (.*)$", headers, re.M)
        assert f"connect-src 'self' wss://irc.weblounge.org/socket https://{members};" in policy, policy
        assert "default-src 'none'" in policy and "frame-ancestors 'none'" in policy, policy

    with subtest("the token logs in over gamja's WebSocket too"):
        session = irc(token=token, websocket="irc.weblounge.org")
        assert " 903 " in session, session
        assert re.search(r"^:grace!\S+ JOIN #lounge", session, re.M), session

    with subtest("Ergo refuses WebSockets from pages on other origins"):
        upgrade = ["--http1.1", "-o", "/dev/null", "-w", "%{http_code}", "-H", "Connection: Upgrade",
                   "-H", "Upgrade: websocket", "-H", "Sec-WebSocket-Version: 13",
                   "-H", "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="]
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
        orphan = ("import sqlite3; db = sqlite3.connect('/var/lib/private/concierge/concierge.db'); "
                  "db.execute(\"INSERT INTO members (account, joined_at, invite_quota) VALUES ('ghost', 0, 0)\"); "
                  f"db.execute(\"INSERT INTO sessions VALUES (x'{session_hash}', 'ghost', 0, 4000000000)\"); db.commit()")
        credenza.succeed(f"${pkgs.python3}/bin/python3 -c {shlex.quote(orphan)}")
        ghosts_browser = Browser("ghost")
        client.succeed(f"printf 'members.weblounge.org\\tFALSE\\t/\\tTRUE\\t0\\t__Host-session\\t{session}\\n' > /tmp/cookies-ghost")
        ghost_token = access_token(ghosts_browser)
        client.fail(irc_command(token=ghost_token))
        body, _ = ergo_api("ns/info", accountName="ghost")
        assert body.strip() == "{\"success\":false}", body

    with subtest("in a browser, a signed-in member opens irc.weblounge.org and lands in #lounge"):
        page = client.succeed(f"webchat-browser {grace['account']} {shlex.quote(grace['password'])} 2>&1")
        assert "#lounge" in page, page

    with subtest("tokens and codes never reach the journal"):
        credenza.succeed("journalctl --sync")
        for secret in [token, revoked, ghost_token, code, verifier]:
            credenza.fail(f"journalctl | grep -qF -- {secret}")
  '';
}

{ vmTest, ... }:

vmTest {
  browsers.invite-browser = ./invite-browser.py;

  script = ''
    erin = {"account": "erin", "password": "lantern-orchard-71"}
    frank = {"account": "frank", "password": "copper-meadow-94"}
    erins_browser = Browser("erin")

    credenza.wait_for_unit("nginx.service")
    credenza.wait_for_unit("concierge.service")
    credenza.wait_for_open_port(8080)
    credenza.wait_for_unit("ergochat.service")
    credenza.wait_for_open_port(8089)

    with subtest("an admin mints a root invite and the invitee opens it over HTTPS"):
        link = concierge_admin("invite", "create", "--root").strip()
        opened = erins_browser.get(link.removeprefix(f"https://{members}"), "-L", "-f")
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

    with subtest("member reset-password locks the old password out of IRC and ends the website session"):
        new_password = concierge_admin("member", "reset-password", "erin").strip()
        client.fail(irc_command(**erin))
        assert erins_browser.get("/", *outcome) == f"303 https://{members}/signin"
        assert " 903 " in irc(account="erin", password=new_password)

    with subtest("the passwords and invite codes never reach the journal"):
        credenza.succeed("journalctl --sync")
        for secret in [erin["password"], frank["password"], new_password, invite_code(link), invite_code(franks_link)]:
            credenza.fail(f"journalctl | grep -qF {secret}")
  '';
}

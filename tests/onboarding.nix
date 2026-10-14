{ vmTest, ... }:

vmTest ''
  import re

  erin = {"account": "erin", "password": "lantern-orchard-71"}
  frank = {"account": "frank", "password": "copper-meadow-94"}
  erins_browser, franks_browser = Browser("erin"), Browser("frank")

  def invite_path(link):
      return link.strip().removeprefix(f"https://{members}")

  def sign_up(browser, path, person, *flags):
      fields = [f"nick={person['account']}", f"email={person['account']}@example.org",
                f"password={person['password']}", f"confirmation={person['password']}"]
      return browser.post(path, *fields, flags=list(flags))

  credenza.wait_for_unit("nginx.service")
  credenza.wait_for_unit("concierge.service")
  credenza.wait_for_open_port(8080)
  credenza.wait_for_unit("ergochat.service")
  credenza.wait_for_open_port(8089)

  with subtest("an admin mints a root invite and the invitee opens it over HTTPS"):
      path = invite_path(concierge_admin("invite", "create", "--root"))
      page = erins_browser.get(path, "-f")
      assert "<h1>You're invited.</h1>" in page, page
      assert 'autocomplete="new-password"' in page, page

  with subtest("signing up answers with the welcome page, uncached, and signs the new member in"):
      response = sign_up(erins_browser, path, erin, "-i")
      headers, page = response.split("\r\n\r\n", 1)
      assert " 200" in headers.splitlines()[0], headers
      assert "cache-control: no-store" in headers.lower(), headers
      assert "<h1>You're in, erin.</h1>" in page, page
      assert '<a class="button button-primary" href="https://irc.weblounge.org/">Open chat</a>' in page, page
      assert "<code>irc.weblounge.org</code>" in page and "<code>6697</code>" in page, page
      assert erin["password"] not in page
      home = erins_browser.get("/", "-f")
      assert "<h1>erin</h1>" in home, home

  with subtest("the used invite link stops working"):
      assert erins_browser.get(path, *outcome) == "404 "

  with subtest("the new member signs in to the website with the password they chose"):
      assert erins_browser.post("/signout").startswith("303 ")
      assert erins_browser.sign_in(**erin) == f"303 https://{members}/"

  with subtest("the new member logs in to IRC with SASL and lands in #lounge"):
      session = irc(**erin)
      assert " 903 " in session, session
      assert re.search(r"^:erin!\S+ JOIN #lounge", session, re.M), session

  with subtest("the new member invites someone, who joins under them"):
      assert erins_browser.post("/invites") == f"303 https://{members}/?invited#invites"
      home = erins_browser.get("/", "-f")
      [link] = re.findall(r'<code class="invite-link">([^<]+)</code>', home)
      assert sign_up(franks_browser, invite_path(link), frank, *outcome) == "200 "
      home = erins_browser.get("/", "-f")
      assert "<li>frank</li>" in home, home
      assert "frank" not in concierge_admin("member", "reconcile")
      assert " 903 " in irc(**frank)

  with subtest("member reset-password locks the old password out of IRC and ends the website session"):
      new_password = concierge_admin("member", "reset-password", "erin").strip()
      client.fail(irc_command(**erin))
      assert erins_browser.get("/", *outcome) == f"303 https://{members}/signin"
      assert " 903 " in irc(account="erin", password=new_password)

  with subtest("the passwords and invite codes never reach the journal"):
      credenza.succeed("journalctl --sync")
      for secret in [erin["password"], frank["password"], new_password, path.rsplit("/", 1)[1]]:
          credenza.fail(f"journalctl | grep -qF {secret}")
''

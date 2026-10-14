{ vmTest, ... }:

vmTest ''
  import html
  import re

  with subtest("members.weblounge.org serves concierge's sign-in page in the shared styles"):
      credenza.wait_for_unit("nginx.service")
      credenza.wait_for_unit("concierge.service")
      credenza.wait_for_open_port(8080)
      page = https("members.weblounge.org", "/signin", "-f")
      assert "Sign in." in page, page
      stylesheet = re.search(r'href="(/assets/members-[A-Z0-9]+\.css)"', page)
      assert stylesheet, page
      css = https("members.weblounge.org", stylesheet[1], "-f")
      assert "--paper:" in css, css

  with subtest("concierge serves the assets its page links, under hashed names cached for good"):
      member_assets = set(re.findall(r'(?:href|src)="(/assets/[^"#]+)', page))
      for asset in member_assets:
          assert re.fullmatch(r"/assets/\w+-[A-Z0-9]{8}\.\w+", asset), asset
          headers = https("members.weblounge.org", asset, "-fI").lower()
          assert "cache-control: public, max-age=31536000, immutable" in headers, headers
      types = {https("members.weblounge.org", asset, "-f", "-o", "/dev/null", "-w", "%{content_type}") for asset in member_assets}
      assert types == {"text/css", "text/javascript", "image/svg+xml"}, (member_assets, types)

  with subtest("the header draws the mark and wordmark from files that have the ids it uses"):
      for asset, fragment in re.findall(r'<use href="(/assets/[^"#]+)#([^"]+)"', page):
          svg = https("members.weblounge.org", asset, "-f")
          assert f'id="{fragment}"' in svg, (asset, svg)

  with subtest("concierge answers health checks"):
      health = https("members.weblounge.org", "/healthz", "-f")
      assert health == "ok", health

  with subtest("concierge's release build carries no live-reload code"):
      assert "livereload" not in page, page
      credenza.fail("grep -aq tower-livereload /proc/$(systemctl show -P MainPID concierge)/exe")

  with subtest("concierge-admin runs concierge as the service does, with its state, secrets, database and Ergo"):
      service_uid = credenza.succeed("ps -o uid= -p $(systemctl show -P MainPID concierge)").strip()
      assert service_uid != "0", service_uid
      credenza.wait_for_open_port(8089)
      status = concierge_admin("status")
      assert status == (
          "public url: https://members.weblounge.org\n"
          f"state directory: /var/lib/concierge (writable by uid {service_uid})\n"
          "session key: 64 bytes\n"
          "database: /var/lib/concierge/concierge.db (migration 20261015120000, 0 members)\n"
          "ergo: http://127.0.0.1:8089/ answers, version 2.19.1\n"
      ), (service_uid, status)
      credenza.fail(concierge_admin_command("no-such-command"))

  with subtest("concierge's database lives in its state directory, private to the service"):
      modes = credenza.succeed("stat -L -c %a /var/lib/private /var/lib/concierge /var/lib/concierge/concierge.db").split()
      assert modes == ["700", "700", "600"], modes

  with subtest("concierge-admin invite create --root prints a link to members.weblounge.org"):
      link = concierge_admin("invite", "create", "--root", "--note", "for the founders")
      assert re.fullmatch(r"https://members\.weblounge\.org/invite/[A-Za-z0-9_-]{22}\n", link), link

  with subtest("concierge-admin member adopt imports an account made through Ergo's API"):
      credenza.wait_for_open_port(8089)
      dana = {"account": "Dana", "password": "an-old-password-of-hers"}
      assert '"success":true' in register(**dana)
      adopted = concierge_admin("member", "adopt", "dana", "--admin")
      assert adopted == "adopted Dana as an admin\n", adopted
      status = concierge_admin("status")
      assert "(migration 20261015120000, 1 members)" in status, status

  with subtest("concierge-admin member grant-invites raises a member's invite quota"):
      granted = concierge_admin("member", "grant-invites", "dana", "3")
      assert granted == "Dana's invite quota is now 8\n", granted
      credenza.fail(concierge_admin_command("member", "grant-invites", "dana", "0"))

  with subtest("concierge-admin member reset-password replaces the IRC password and prints the new one"):
      password = concierge_admin("member", "reset-password", "dana").strip()
      assert re.fullmatch("[a-z2-9]{26}", password), password
      client.fail(irc_command(**dana))
      session = irc(account="Dana", password=password)
      assert " 903 " in session, session

  browser = Browser("dana")

  with subtest("an adopted member signs in to the website with their IRC password"):
      signed_in = browser.sign_in("dana", password)
      assert signed_in == f"303 https://{members}/", signed_in
      home = browser.get("/", "-f")
      assert "Pull up a chair, Dana." in home, home
      assert home.count("<circle ") == 53, home
      clients = browser.get("/clients", "-f")
      assert "<code>irc.weblounge.org</code>" in clients and "<code>6697</code>" in clients, clients

  with subtest("a member changes their email and news setting on the Account tab"):
      saved = browser.post("/settings", "email=dana@example.org", "news=on")
      assert saved == f"303 https://{members}/account?saved#email-and-news", saved
      tab = browser.get("/account?saved", "-f")
      assert "Saved." in tab and 'value="dana@example.org"' in tab, tab
      assert 'type="checkbox" checked' in tab, tab

  with subtest("a member changes their password: IRC takes only the new one, and their other sessions end"):
      laptop = Browser("dana-laptop")
      assert laptop.sign_in("dana", password).startswith("303 ")
      old_password, password = password, "a-passphrase-dana-chose"
      changed = browser.post("/password", f"current={old_password}", f"password={password}", f"confirmation={password}")
      assert changed == f"303 https://{members}/account?password#change-password", changed
      assert "Password changed." in browser.get("/account?password", "-f")
      after = laptop.get("/", *outcome)
      assert after == f"303 https://{members}/signin", after
      client.fail(irc_command(account="Dana", password=old_password))
      session = irc(account="Dana", password=password)
      assert " 903 " in session, session

  with subtest("a member turns always-on off on the IRC tab, and Ergo has it"):
      saved = browser.post("/irc-settings", "auto_away=on")
      assert saved == f"303 https://{members}/irc?saved", saved
      body, _ = ergo_api("ns/saget", accountName="Dana")
      settings = json.loads(body)
      assert (settings["alwaysOn"], settings["autoAway"]) == ("off", "default"), body
      tab = browser.get("/irc?saved", "-f")
      assert '<input name="always_on" type="checkbox">' in tab, tab
      assert '<input name="auto_away" type="checkbox" checked>' in tab, tab
      assert "Both on by default." in tab, tab

  with subtest("the invite tree lists a new member under whoever invited them"):
      assert browser.post("/invites") == f"303 https://{members}/invites?invited"
      [link] = re.findall(r'<code class="invite-link">([^<]+)</code>', browser.get("/invites", "-f"))
      fields = ["nick=ivy", "email=ivy@example.org", "password=ivy-chose-this-one", "confirmation=ivy-chose-this-one"]
      assert Browser("ivy").post(link.removeprefix(f"https://{members}"), *fields) == "200 "
      tree = browser.get("/invites", "-f")
      assert re.search(r'aria-current="true">Dana</strong>.*?<ul>\s*<li>\s*<span>ivy</span>', tree, re.S), tree

  with subtest("the client certificate guide's commands attach a certificate that SASL EXTERNAL logs in with"):
      guide = browser.get("/clients", "-f")
      generate, fingerprint = [html.unescape(block) for block in re.findall(r"<pre><code>(.*?)</code></pre>", guide, re.S)[:2]]
      assert "/CN=Dana" in generate, generate
      client.succeed(f"mkdir -p /tmp/cert && cd /tmp/cert && {generate}")
      certfp = client.succeed(f"cd /tmp/cert && {fingerprint}").strip()
      assert re.fullmatch("[0-9a-f]{64}", certfp), certfp
      added = irc(f"PRIVMSG NickServ :CERT ADD {certfp}", account="Dana", password=password)
      assert "NOTICE Dana :Certificate fingerprint successfully added" in added, added
      client.succeed("cat /tmp/cert/weblounge.key /tmp/cert/weblounge.crt > /tmp/cert/weblounge.pem")
      session = irc(account="Dana", cert="/tmp/cert/weblounge.pem")
      assert " 903 " in session and re.search(r" 900 \S+ .* :You are now logged in as Dana", session), session

  with subtest("signing out ends the session"):
      signed_out = browser.post("/signout")
      assert signed_out == f"303 https://{members}/signin", signed_out
      after = browser.get("/", *outcome)
      assert after == f"303 https://{members}/signin", after

  with subtest("concierge-admin member reset-password ends the member's website sessions"):
      assert browser.sign_in("dana", password).startswith("303 ")
      assert browser.get("/", *outcome).startswith("200 ")
      new_password = concierge_admin("member", "reset-password", "dana").strip()
      after = browser.get("/", *outcome)
      assert after == f"303 https://{members}/signin", after
      assert browser.sign_in("dana", new_password).startswith("303 ")

  with subtest("the passwords never reach the journal"):
      credenza.succeed("journalctl --sync")
      for secret in [old_password, password, new_password]:
          credenza.fail(f"journalctl | grep -qF {secret}")
''

{ vmTest, ... }:

vmTest ''
  import re

  alice = {"account": "alice", "password": "correct-horse"}
  bob = {"account": "bob", "password": "battery-staple"}
  carol = {"account": "carol", "password": "tr0ub4dor-and-3"}

  with subtest("ergo serves IRC over TLS on 6697 with irc.weblounge.org's certificate"):
      credenza.wait_for_unit("ergochat.service")
      credenza.wait_for_open_port(6697)
      names = client.succeed(
          "openssl s_client -connect credenza:6697 -servername irc.weblounge.org </dev/null 2>/dev/null"
          " | openssl x509 -noout -ext subjectAltName"
      )
      assert "DNS:irc.weblounge.org" in names, names
      credenza.fail("ss -Htln | grep -q ':6667 '")

  with subtest("a renewed certificate reaches ergo on reload, without a restart"):
      fingerprint = "openssl s_client -connect credenza:6697 </dev/null 2>/dev/null | openssl x509 -noout -fingerprint"
      before = client.succeed(fingerprint)
      pid = credenza.succeed("systemctl show -P MainPID ergochat")
      credenza.succeed("systemctl restart acme-irc.weblounge.org.service")
      credenza.succeed("systemctl try-reload-or-restart ergochat")
      client.wait_until_succeeds(f"{fingerprint} | grep -vqF '{before.strip()}'", timeout=10)
      assert credenza.succeed("systemctl show -P MainPID ergochat") == pid

  with subtest("irc.weblounge.org in a browser redirects to the landing page"):
      credenza.wait_for_unit("nginx.service")
      redirect = https("irc.weblounge.org", "/", "-o", "/dev/null", "-w", "%{redirect_url}")
      assert redirect == "https://weblounge.org/", redirect

  with subtest("a client that doesn't log in with SASL is turned away, even from loopback"):
      for reply in [irc(), irc(machine=credenza, host="localhost")]:
          assert " 001 " not in reply and re.search(r"^ERROR .*SASL", reply, re.M), reply

  with subtest("default.yaml's example oper and WebIRC gateway are left out"):
      credenza.fail("grep -qE '^opers:|webirc' /etc/ergo.yaml")

  with subtest("ergo's API answers on credenza's loopback, only to the generated token"):
      body, status = ergo_api("status")
      assert status == "200" and '"success":true' in body, (status, body)
      _, status = ergo_api("status", token="not-the-token")
      assert status == "401", status
      assert_loopback_only(8089, "/v1/status")

  with subtest("the API token stays out of the Nix store and readable by root alone"):
      modes = credenza.succeed("stat -c '%a %U' /var/lib/weblounge-secrets /var/lib/weblounge-secrets/ergo-api-token")
      assert modes == "700 root\n600 root\n", modes
      credenza.fail(f"{{ systemctl cat ergochat; cat /etc/ergo.yaml; }} | grep -qF {ergo_api_token()}")

  with subtest("the token survives a restart of the secrets service and ergo"):
      token = ergo_api_token()
      credenza.succeed("systemctl restart weblounge-secrets ergochat")
      credenza.wait_for_open_port(8089)
      _, status = ergo_api("status", token=token)
      assert status == "200", status

  with subtest("an account created through the API logs in over TLS with SASL"):
      body = register(**alice)
      assert '"success":true' in body, body
      session = irc(**alice)
      assert " 903 " in session and " 001 alice " in session, session

  with subtest("NickServ won't register new accounts"):
      reply = irc("PRIVMSG NickServ :REGISTER battery-staple", **alice)
      assert "NOTICE alice :This command has been disabled" in reply, reply

  with subtest("logging in shows weblounge's MOTD"):
      assert ":- Pull up a chair and make yourself at home." in session, session

  with subtest("members' addresses are cloaked under weblounge"):
      whois = irc("WHOIS alice", **alice)
      assert re.search(r" 311 alice alice \S+ \S+\.weblounge ", whois), whois

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
      renewed = {**carol, "password": "a-longer-generated-one"}
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
''

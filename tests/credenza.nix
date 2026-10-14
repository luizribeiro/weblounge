{ pkgs, vmTest, ... }:

vmTest ''
  alice = {"account": "alice", "password": "correct-horse"}
  bob = {"account": "bob", "password": "battery-staple"}
  carol = {"account": "carol", "password": "tr0ub4dor-and-3"}

  with subtest("nginx redirects to HTTPS and serves the parking page"):
      credenza.wait_for_unit("nginx.service")
      credenza.wait_for_open_port(443)
      client.wait_for_unit("multi-user.target")
      redirect = client.succeed("curl -s -o /dev/null -w '%{redirect_url}' http://credenza/")
      assert redirect == "https://credenza/", redirect
      page = client.succeed("curl -sfk https://credenza/")
      assert "<title>weblounge.org</title>" in page, page

  with subtest("the stylesheet, script and favicon the page references are served"):
      assets = re.findall(r'(?:href|src)="(/[^"]+)"', page)
      responses = {
          client.succeed(f"curl -sk -o /dev/null -w '%{{http_code}} %{{content_type}}' https://credenza{asset}")
          for asset in assets
      }
      assert responses == {"200 text/css", "200 text/javascript", "200 image/svg+xml"}, (assets, responses)

  with subtest("the page and its assets are cached briefly and revalidated by ETag"):
      for path in ["/", *assets]:
          headers = client.succeed(f"curl -sfkI https://credenza{path}").lower()
          assert "cache-control: max-age=3600" in headers, (path, headers)
          assert "etag:" in headers, (path, headers)

  with subtest("the release build carries no live-reload code"):
      for path in ["/", *assets]:
          client.succeed(f"curl -sfk -o body https://credenza{path} && ! grep -qE 'EventSource|/esbuild' body")

  with subtest("nginx logs requests without the client's address, even when they fail"):
      client.succeed("curl -sk -o /dev/null https://credenza/no-such-page")
      credenza.succeed("grep -q 'GET /no-such-page HTTP/[0-9.]*\" 404' /var/log/nginx/access.log")
      assert_client_address_absent("{ cat /var/log/nginx/*; journalctl -u nginx; }")

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
      redirect = https("irc.weblounge.org", "/", "-o", "/dev/null", "-w", "%{redirect_url}")
      assert redirect == "https://weblounge.org/", redirect

  with subtest("a client that doesn't log in with SASL is turned away, even from loopback"):
      for reply in [irc(), irc(machine=credenza, host="localhost")]:
          assert " 001 " not in reply and re.search(r"^ERROR .*SASL", reply, re.M), reply

  with subtest("default.yaml's example oper and WebIRC gateway are left out"):
      credenza.fail("grep -qE '^opers:|webirc' /etc/ergo.yaml")

  with subtest("ergo's API answers on credenza's loopback, only to the generated token"):
      token = credenza.succeed("cat /var/lib/weblounge-secrets/ergo-api-token").strip()
      body, status = ergo_api("status", token)
      assert status == "200" and '"success":true' in body, (status, body)
      _, status = ergo_api("status", "not-the-token")
      assert status == "401", status
      assert_loopback_only(8089, "/v1/status")

  with subtest("the API token stays out of the Nix store and readable by root alone"):
      modes = credenza.succeed("stat -c '%a %U' /var/lib/weblounge-secrets /var/lib/weblounge-secrets/ergo-api-token")
      assert modes == "700 root\n600 root\n", modes
      credenza.fail(f"{{ systemctl cat ergochat; cat /etc/ergo.yaml; }} | grep -qF {token}")

  with subtest("the token survives a restart of the secrets service and ergo"):
      credenza.succeed("systemctl restart weblounge-secrets ergochat")
      credenza.wait_for_open_port(8089)
      _, status = ergo_api("status", token)
      assert status == "200", status

  with subtest("an account created through the API logs in over TLS with SASL"):
      body = register(token, **alice)
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
      register(token, **bob)
      seen = irc("NAMES #lounge", "WHOIS alice", **bob)
      assert re.search(r" 353 bob = #lounge :.*\balice\b", seen), seen
      assert " 301 bob alice " in seen, seen

  with subtest("members may delete their own messages from history"):
      reply = irc("PRIVMSG HistServ :DELETE #lounge no-such-message", **alice)
      assert "NOTICE alice :Message not found" in reply, reply

  with subtest("ergo advertises web push for mobile clients"):
      assert "VAPID=" in session, session

  with subtest("the API changes a member's password, and only the new one logs in"):
      register(token, **carol)
      renewed = {**carol, "password": "a-longer-generated-one"}
      body, _ = ergo_api("ns/passwd", token, accountName=carol["account"], passphrase=renewed["password"])
      assert json.loads(body)["success"], body
      client.fail(irc_command(**carol))
      renewed_session = irc(**renewed)
      assert " 903 " in renewed_session, renewed_session

  with subtest("the API reads and changes a member's always-on setting"):
      body, _ = ergo_api("ns/saget", token, accountName=carol["account"])
      assert json.loads(body)["alwaysOn"] == "default", body
      body, _ = ergo_api("ns/saset", token, accountName=carol["account"], alwaysOn="off")
      assert json.loads(body)["alwaysOn"] == "off", body
      body, _ = ergo_api("ns/saget", token, accountName=carol["account"])
      assert json.loads(body)["alwaysOn"] == "off", body

  with subtest("members.weblounge.org serves concierge's page in the shared styles"):
      credenza.wait_for_unit("concierge.service")
      credenza.wait_for_open_port(8080)
      page = https("members.weblounge.org", "/", "-f")
      assert "Members' entrance." in page, page
      stylesheet = re.search(r'href="(/assets/members-[A-Z0-9]+\.css)"', page)
      assert stylesheet, page
      css = https("members.weblounge.org", stylesheet[1], "-f")
      assert "--paper:" in css, css

  with subtest("concierge serves the assets its page links, under hashed names cached for good"):
      member_assets = re.findall(r'href="(/assets/[^"]+)"', page)
      for asset in member_assets:
          assert re.fullmatch(r"/assets/\w+-[A-Z0-9]{8}\.\w+", asset), asset
          headers = https("members.weblounge.org", asset, "-fI").lower()
          assert "cache-control: public, max-age=31536000, immutable" in headers, headers
      types = {https("members.weblounge.org", asset, "-f", "-o", "/dev/null", "-w", "%{content_type}") for asset in member_assets}
      assert types == {"text/css", "image/svg+xml"}, (member_assets, types)

  with subtest("concierge answers health checks"):
      health = https("members.weblounge.org", "/healthz", "-f")
      assert health == "ok", health

  with subtest("concierge's release build carries no live-reload code"):
      assert "livereload" not in page, page
      credenza.fail("grep -aq tower-livereload /proc/$(systemctl show -P MainPID concierge)/exe")

  with subtest("concierge listens on credenza's loopback only"):
      assert_loopback_only(8080, "/healthz")

  with subtest("concierge-admin runs concierge as the service does, with its state and session key"):
      service_uid = credenza.succeed("ps -o uid= -p $(systemctl show -P MainPID concierge)").strip()
      assert service_uid != "0", service_uid
      status = credenza.succeed("su - luiz -c 'concierge-admin status'")
      assert status == (
          "public url: https://members.weblounge.org\n"
          f"state directory: /var/lib/concierge (writable by uid {service_uid})\n"
          "session key: 64 bytes\n"
      ), (service_uid, status)
      credenza.fail("su - luiz -c 'concierge-admin no-such-command'")

  with subtest("the session key stays out of the Nix store"):
      key = credenza.succeed("cat /var/lib/weblounge-secrets/session-key").strip()
      credenza.fail(f"{{ systemctl cat concierge; cat $(readlink -f $(command -v concierge-admin)); }} | grep -qF {key}")

  with subtest("sshd only accepts keys"):
      credenza.wait_for_unit("sshd.service")
      config = credenza.succeed("sshd -T").lower()
      assert "passwordauthentication no" in config, config
      assert "permitrootlogin no" in config, config

  with subtest("admins log in with their key and get passwordless sudo"):
      credenza.succeed("id -nG luiz | grep -qw wheel")
      credenza.succeed("grep -q cardno: /etc/ssh/authorized_keys.d/luiz")
      credenza.succeed("su - luiz -c 'sudo -n true'")
      credenza.succeed("getent shadow luiz | cut -d: -f2 | grep -q '^\\$y\\$'")

  with subtest("the VPS's NIC comes up with its static address and gateway"):
      credenza.wait_until_succeeds("ip -4 addr show | grep -q 'inet 107.173.231.206/25'", timeout=60)
      credenza.succeed("ip route show default | grep -q 'via 107.173.231.129'")

  with subtest("firewall drops ports nothing should listen on"):
      credenza.succeed("systemd-run --unit=stray-listener -- ${pkgs.python3}/bin/python3 -m http.server 9000")
      credenza.wait_for_open_port(9000)
      client.wait_for_unit("multi-user.target")
      client.succeed("nc -z credenza 22")
      client.fail("curl -sf --max-time 3 http://credenza:9000/")
''

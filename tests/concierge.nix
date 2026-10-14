{ vmTest, ... }:

vmTest ''
  import re

  def concierge_admin_command(*args):
      return f"su - luiz -c {shlex.quote(shlex.join(['concierge-admin', *args]))}"

  def concierge_admin(*args):
      return credenza.succeed(concierge_admin_command(*args))

  with subtest("members.weblounge.org serves concierge's page in the shared styles"):
      credenza.wait_for_unit("nginx.service")
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

  with subtest("concierge-admin runs concierge as the service does, with its state, secrets, database and Ergo"):
      service_uid = credenza.succeed("ps -o uid= -p $(systemctl show -P MainPID concierge)").strip()
      assert service_uid != "0", service_uid
      credenza.wait_for_open_port(8089)
      status = concierge_admin("status")
      assert status == (
          "public url: https://members.weblounge.org\n"
          f"state directory: /var/lib/concierge (writable by uid {service_uid})\n"
          "session key: 64 bytes\n"
          "database: /var/lib/concierge/concierge.db (migration 20261014120000, 0 members)\n"
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
      assert "(migration 20261014120000, 1 members)" in status, status

  with subtest("concierge-admin member reset-password replaces the IRC password and prints the new one"):
      password = concierge_admin("member", "reset-password", "dana").strip()
      assert re.fullmatch("[a-z2-9]{26}", password), password
      client.fail(irc_command(**dana))
      session = irc(account="Dana", password=password)
      assert " 903 " in session, session

  with subtest("the printed password never reaches the journal"):
      credenza.succeed("journalctl --sync")
      credenza.fail(f"journalctl | grep -qF {password}")
''

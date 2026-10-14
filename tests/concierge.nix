{ vmTest, ... }:

vmTest ''
  import re

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
''

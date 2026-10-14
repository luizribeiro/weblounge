{ pkgs, vmTest, ... }:

vmTest ''
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
      client.succeed("nc -z credenza 22")
      client.fail("curl -sf --max-time 3 http://credenza:9000/")
''

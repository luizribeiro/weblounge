{ vmTest, ... }:

let
  cert = "irc.weblounge.org";
  attempted = "/var/lib/acme/.attempted-${cert}";
in
vmTest {
  # One certificate's order, which the VM otherwise leaves off, stands in
  # for one the CA refuses the first time, the way it does while a new name
  # hasn't reached every nameserver.
  credenza = { lib, ... }: {
    systemd.services."acme-order-renew-${cert}" = {
      enable = lib.mkForce true;
      script = lib.mkForce ''
        [ -e ${attempted} ] && exit 0
        touch ${attempted}
        exit 10
      '';
      serviceConfig.RestartSec = lib.mkForce 1;
    };
  };

  script = ''
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

    with subtest("a certificate order the CA refuses is tried again, not left for the daily renewal"):
        unit = "acme-order-renew-${cert}.service"
        credenza.wait_until_succeeds("test -e ${attempted}", timeout=60)
        credenza.wait_until_succeeds(f"systemctl show -P Result {unit} | grep -qx success", timeout=60)
        assert credenza.succeed(f"systemctl show -P NRestarts {unit}").strip() == "1"
  '';
}

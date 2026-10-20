{ vmTest, ... }:

let
  cert = "files.weblounge.org";
  attempted = "/var/lib/acme/.attempted-${cert}";
in
vmTest {
  # The deploy checks include the mailer's login.
  mail = true;

  # One certificate's order, which the VM otherwise leaves off, stands in
  # for one the CA refuses the first time, the way it does while a new name
  # hasn't reached every nameserver. It's the first of the ACME account's
  # certificates by name, whose order the others' wait for.
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
}

{ vmTest, ... }:

let
  attempted = "/var/lib/acme/.attempted";
in
vmTest {
  # The deploy checks include the mailer's login.
  mail = true;

  # One certificate's order, which the VM otherwise leaves off, stands in
  # for one the CA refuses the first time, the way it does while a new name
  # hasn't reached every nameserver. It's the first of the ACME account's
  # certificates by name, whose order the others' wait for, so a new name
  # that sorts before it doesn't leave the stand-in waiting on an order the
  # VM never runs.
  credenza = { config, lib, ... }:
    let
      cert = lib.head (lib.attrNames config.security.acme.certs);
    in
    {
      systemd.services."acme-order-renew-${cert}" = {
        enable = lib.mkForce true;
        script = lib.mkForce ''
          [ -e ${attempted} ] && exit 0
          echo ${cert} > ${attempted}
          exit 10
        '';
        serviceConfig.RestartSec = lib.mkForce 1;
      };
    };
}

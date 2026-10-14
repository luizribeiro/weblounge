{ config, lib, ... }:

# Machines other than production can't pass the CA's challenges, since DNS
# points at the real server, and every failed attempt counts against Let's
# Encrypt's rate limits. They keep the self-signed certificates instead.
let
  disablePerCert = unitName: lib.mapAttrs'
    (cert: _: lib.nameValuePair (unitName cert) { enable = false; })
    config.security.acme.certs;
in
{
  # Where nixpkgs' ACME module keeps the CA it signs the stand-ins with.
  weblounge.selfSignedCa = "/var/lib/acme/.minica/cert.pem";

  systemd.services = disablePerCert (cert: "acme-order-renew-${cert}");
  systemd.timers = disablePerCert (cert: "acme-renew-${cert}");
}

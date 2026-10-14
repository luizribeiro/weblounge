# Ergo as credenza runs it, for VM tests that bring their own server:
# credenza's settings, minus its listeners and their certificates, under
# the test's. Takes the test's settings and returns a NixOS module, which
# needs pkgs with our overlay for our Ergo and its config builder.
{ lib, settings }:

testSettings: { pkgs, ... }:
let
  portable = settings // { server = removeAttrs settings.server [ "listeners" ]; };
in
{
  services.ergochat = {
    enable = true;
    configFile = pkgs.weblounge.ergoConfig (lib.recursiveUpdate portable testSettings);
  };
  # default.yaml keeps the database and lock file in the working directory.
  systemd.services.ergochat.serviceConfig.WorkingDirectory = "/var/lib/ergo";
}

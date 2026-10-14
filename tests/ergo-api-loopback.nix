{ pkgs, weblounge }:

let
  inherit (pkgs) lib;

  rejects = listener:
    let
      host = weblounge.hosts.credenza.extendModules {
        modules = [{ weblounge.ergo.settings.api.listener = lib.mkForce listener; }];
      };
    in
    lib.any (check: !check.assertion && lib.hasInfix "loopback" check.message)
      host.config.assertions;

  shouldReject = {
    "127.0.0.1:8089" = false;
    "[::1]:8089" = false;
    "localhost:8089" = false;
    ":8089" = true;
    "0.0.0.0:8089" = true;
    "[::]:8089" = true;
    "107.173.231.206:8089" = true;
    "127.0.0.1.example.com:8089" = true;
  };

  misjudged = lib.attrNames
    (lib.filterAttrs (listener: expected: rejects listener != expected) shouldReject);
in
pkgs.runCommand "ergo-api-loopback" { } (
  if misjudged == [ ] then "touch $out"
  else "echo 'misjudged API listeners: ${toString misjudged}' >&2; exit 1"
)

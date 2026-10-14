{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  expected = {
    domain = "weblounge.org";
    ttl = 3600;
    records = [
      [ "A" "credenza" "107.173.231.206" ]
      [ "CNAME" "irc" "credenza.weblounge.org." ]
      [ "CNAME" "members" "credenza.weblounge.org." ]
      [ "A" "@" "107.173.231.206" ]
      [ "CNAME" "www" "credenza.weblounge.org." ]
      [ "MX" "@" 0 "." ]
      [ "TXT" "@" "v=spf1 -all" ]
      [ "TXT" "_dmarc" "v=DMARC1; p=reject" ]
    ];
  };

  zoneServing = name: import ../dns/zone.nix {
    inherit lib;
    hosts.ottoman.config = {
      weblounge = { domain = "weblounge.org"; publicIPv4 = "192.0.2.1"; };
      networking.fqdn = "ottoman.weblounge.org";
      services.nginx.virtualHosts.${name}.serverAliases = [ ];
    };
  };

  refused = !(builtins.tryEval (builtins.deepSeq (zoneServing "weblounge.example").records null)).success;

  failures =
    lib.optional (weblounge.zone != expected)
      "expected ${builtins.toJSON expected}, got ${builtins.toJSON weblounge.zone}"
    ++ lib.optional (!refused)
      "a name outside weblounge.org made it into the zone";
in
pkgs.runCommand "dns" { } (
  if failures == [ ] then "touch $out"
  else "echo ${lib.escapeShellArg (lib.concatLines failures)} >&2; exit 1"
)

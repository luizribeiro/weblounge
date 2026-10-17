{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  zoneServing = name: import ../dns/zone.nix {
    inherit lib;
    hosts.ottoman.config = {
      weblounge = { domain = "weblounge.org"; publicIPv4 = "192.0.2.1"; };
      networking.fqdn = "ottoman.weblounge.org";
      services.nginx.virtualHosts.${name}.serverAliases = [ ];
    };
  };
in
{
  testZoneHasTheHostsNamesAndMailPolicy = {
    expr = weblounge.zone;
    expected = {
      domain = "weblounge.org";
      ttl = 3600;
      records = [
        [ "A" "credenza" "107.173.231.206" ]
        [ "CNAME" "irc" "credenza.weblounge.org." ]
        [ "CNAME" "join" "credenza.weblounge.org." ]
        [ "CNAME" "members" "credenza.weblounge.org." ]
        [ "A" "@" "107.173.231.206" ]
        [ "CNAME" "www" "credenza.weblounge.org." ]
        [ "MX" "@" 0 "." ]
        [ "TXT" "@" "v=spf1 -all" ]
        [ "TXT" "_dmarc" "v=DMARC1; p=reject" ]
        [ "TXT" "_da-verify-bbbfa40b313de02f113fc58ed97fecf1" "domain-verified" ]
      ];
    };
  };

  testNameOutsideTheDomainIsRefused = {
    expr = (builtins.tryEval (builtins.deepSeq (zoneServing "weblounge.example").records null)).success;
    expected = false;
  };
}

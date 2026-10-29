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
        [ "CNAME" "beta" "credenza.weblounge.org." ]
        [ "CNAME" "files" "credenza.weblounge.org." ]
        [ "CNAME" "irc" "credenza.weblounge.org." ]
        [ "CNAME" "join" "credenza.weblounge.org." ]
        [ "CNAME" "members" "credenza.weblounge.org." ]
        [ "A" "@" "107.173.231.206" ]
        [ "CNAME" "www" "credenza.weblounge.org." ]
        [ "MX" "@" 10 "shadow.mxrouting.net." ]
        [ "MX" "@" 20 "shadow-relay.mxrouting.net." ]
        [ "TXT" "@" "v=spf1 include:mxroute.com -all" ]
        [ "TXT" "x._domainkey" "v=DKIM1;k=rsa;p=MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEApwyfGWLuUrgBWuiOXrZjZgeDBuyZ3Zxw/WX6M42Qb6LlmWLRFGUqQgnz+5YnzAmz0yYOp6vjtNy8ZICfAJ+PfqMWH5Ym2DcxsLyTGct8GItOtL1Hc51a9fEtcmsbcRbczb+0ya0ur3qkQ7U3mz/lxvUJ0CMe39wrRe/cKlX2SrXWX+h/aCIhR78WN3il0MDjp1bZ5AfdqSX2PoZyx4mtyCssH8qv1ISnHrKat+L5SVVr0VCBse/wz3bKavYbxPS9A8+tA5LM4Fg3TIoOqY9dFsF9hpNZgqkWbyfUhXY0rEfLbMoSUmf+IhmOmA0vR4sbe9kTVgx/ohw6d4XHjpOi2QIDAQAB" ]
        [ "TXT" "_dmarc" "v=DMARC1; p=none" ]
        [ "TXT" "_da-verify-bbbfa40b313de02f113fc58ed97fecf1" "domain-verified" ]
        [ "TXT" "@" "google-site-verification=wb1kjFnT5ogH9jj24q8peN6cp6jNGkUArKC2KLZtbc8" ]
      ];
    };
  };

  testNameOutsideTheDomainIsRefused = {
    expr = (builtins.tryEval (builtins.deepSeq (zoneServing "weblounge.example").records null)).success;
    expected = false;
  };
}

# Every host's own name and the names its nginx serves, plus the records
# that route the domain's mail through MXroute.
{ lib, hosts }:

let
  configs = map (host: host.config) (lib.attrValues hosts);
  inherit ((lib.head configs).weblounge) domain;

  relative = name:
    if name == domain then "@"
    else
      lib.throwIfNot (lib.hasSuffix ".${domain}" name)
        "${name} is outside ${domain}, so the zone can't publish it"
        (lib.removeSuffix ".${domain}" name);

  servedNames = config: lib.concatLists (lib.mapAttrsToList
    (name: vhost: [ name ] ++ vhost.serverAliases)
    config.services.nginx.virtualHosts);

  hostRecords = config:
    let
      inherit (config.networking) fqdn;
      recordFor = name:
        if name == domain || name == fqdn
        then [ "A" (relative name) config.weblounge.publicIPv4 ]
        else [ "CNAME" (relative name) "${fqdn}." ];
    in
    map recordFor (lib.unique ([ fqdn ] ++ servedNames config));

  # MXroute's DKIM key, from its panel; it changes only if MXroute
  # rotates it. DMARC starts at p=none while delivery is new.
  mxroute = [
    [ "MX" "@" 10 "shadow.mxrouting.net." ]
    [ "MX" "@" 20 "shadow-relay.mxrouting.net." ]
    [ "TXT" "@" "v=spf1 include:mxroute.com -all" ]
    [ "TXT" "x._domainkey" "v=DKIM1;k=rsa;p=MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEApwyfGWLuUrgBWuiOXrZjZgeDBuyZ3Zxw/WX6M42Qb6LlmWLRFGUqQgnz+5YnzAmz0yYOp6vjtNy8ZICfAJ+PfqMWH5Ym2DcxsLyTGct8GItOtL1Hc51a9fEtcmsbcRbczb+0ya0ur3qkQ7U3mz/lxvUJ0CMe39wrRe/cKlX2SrXWX+h/aCIhR78WN3il0MDjp1bZ5AfdqSX2PoZyx4mtyCssH8qv1ISnHrKat+L5SVVr0VCBse/wz3bKavYbxPS9A8+tA5LM4Fg3TIoOqY9dFsF9hpNZgqkWbyfUhXY0rEfLbMoSUmf+IhmOmA0vR4sbe9kTVgx/ohw6d4XHjpOi2QIDAQAB" ]
    [ "TXT" "_dmarc" "v=DMARC1; p=none" ]
    [ "TXT" "_da-verify-bbbfa40b313de02f113fc58ed97fecf1" "domain-verified" ]
  ];

  # Gmail Postmaster Tools reports our sender reputation at Gmail only to
  # an owner it has verified.
  gmailPostmaster = [
    [ "TXT" "@" "google-site-verification=wb1kjFnT5ogH9jj24q8peN6cp6jNGkUArKC2KLZtbc8" ]
  ];
in
{
  inherit domain;
  # deSEC refuses anything shorter.
  ttl = 3600;
  records = lib.concatMap hostRecords configs ++ mxroute ++ gmailPostmaster;
}

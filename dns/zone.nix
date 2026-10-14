# Every host's own name and the names its nginx serves, plus records that
# say the domain sends and receives no mail.
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

  noMail = [
    [ "MX" "@" 0 "." ]
    [ "TXT" "@" "v=spf1 -all" ]
    [ "TXT" "_dmarc" "v=DMARC1; p=reject" ]
  ];
in
{
  inherit domain;
  # deSEC refuses anything shorter.
  ttl = 3600;
  records = lib.concatMap hostRecords configs ++ noMail;
}

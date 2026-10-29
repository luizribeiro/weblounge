{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;
  inherit (weblounge.hosts.credenza) config;

  envOf = host: prefix: lib.findFirst (lib.hasPrefix prefix) null
    host.systemd.services.concierge.serviceConfig.Environment;

  namesOf = host: {
    virtualHosts = lib.mapAttrs (_: vhost: vhost.serverAliases) host.services.nginx.virtualHosts;
    conciergeUrl = envOf host "CONCIERGE_PUBLIC_URL=";
    joinUrl = envOf host "CONCIERGE_JOIN_URL=";
  };
in
{
  testProductionServesWebloungeOrg = {
    expr = namesOf config;
    expected = {
      virtualHosts = {
        "weblounge.org" = [ "www.weblounge.org" "credenza.weblounge.org" ];
        "beta.weblounge.org" = [ ];
        "files.weblounge.org" = [ ];
        "irc.weblounge.org" = [ ];
        "members.weblounge.org" = [ ];
        "join.weblounge.org" = [ ];
      };
      conciergeUrl = "CONCIERGE_PUBLIC_URL=https://members.weblounge.org";
      joinUrl = "CONCIERGE_JOIN_URL=https://join.weblounge.org";
    };
  };

  testVmServesWebloungeLocalhost = {
    expr = namesOf config.virtualisation.vmVariant;
    expected = {
      virtualHosts = {
        "weblounge.localhost" = [ "www.weblounge.localhost" "credenza.weblounge.localhost" ];
        "beta.weblounge.localhost" = [ ];
        "files.weblounge.localhost" = [ ];
        "irc.weblounge.localhost" = [ ];
        "members.weblounge.localhost" = [ ];
        "join.weblounge.localhost" = [ ];
      };
      conciergeUrl = "CONCIERGE_PUBLIC_URL=https://members.weblounge.localhost:10443";
      joinUrl = "CONCIERGE_JOIN_URL=https://join.weblounge.localhost:10443";
    };
  };
}

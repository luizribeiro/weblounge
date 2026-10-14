{ pkgs, weblounge }:

let
  inherit (pkgs) lib;
  inherit (weblounge.hosts.credenza) config;

  namesOf = host: {
    virtualHosts = lib.mapAttrs (_: vhost: vhost.serverAliases) host.services.nginx.virtualHosts;
    conciergeUrl = lib.findFirst (lib.hasPrefix "CONCIERGE_PUBLIC_URL=") null
      host.systemd.services.concierge.serviceConfig.Environment;
  };

  expected = {
    production = {
      virtualHosts = {
        "weblounge.org" = [ "www.weblounge.org" "credenza.weblounge.org" ];
        "irc.weblounge.org" = [ ];
        "members.weblounge.org" = [ ];
      };
      conciergeUrl = "CONCIERGE_PUBLIC_URL=https://members.weblounge.org";
    };
    vm = {
      virtualHosts = {
        "weblounge.localhost" = [ "www.weblounge.localhost" "credenza.weblounge.localhost" ];
        "irc.weblounge.localhost" = [ ];
        "members.weblounge.localhost" = [ ];
      };
      conciergeUrl = "CONCIERGE_PUBLIC_URL=https://members.weblounge.localhost:10443";
    };
  };

  actual = {
    production = namesOf config;
    vm = namesOf config.virtualisation.vmVariant;
  };
in
pkgs.runCommand "public-names" { } (
  if actual == expected then "touch $out"
  else "echo ${lib.escapeShellArg "expected ${builtins.toJSON expected}, got ${builtins.toJSON actual}"} >&2; exit 1"
)

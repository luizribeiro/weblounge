{ config, pkgs, ... }:

{
  imports = [ ./nginx.nix ];

  services.nginx.virtualHosts."weblounge.org" = {
    default = true;
    serverAliases = [
      "www.weblounge.org"
      "irc.weblounge.org"
      config.networking.fqdn
    ];
    enableACME = true;
    forceSSL = true;
    root = "${pkgs.weblounge.web}/landing";
  };
}

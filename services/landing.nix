{ config, pkgs, ... }:

let
  inherit (config.weblounge) domain;
in
{
  imports = [ ./nginx.nix ];

  services.nginx.virtualHosts.${domain} = {
    default = true;
    serverAliases = [
      "www.${domain}"
      config.networking.fqdn
    ];
    enableACME = true;
    forceSSL = true;
    root = "${pkgs.weblounge.web}/landing";
    # Asset names carry no hash, so browsers must recheck them soon.
    extraConfig = "expires 1h;";
  };
}

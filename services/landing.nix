{ config, pkgs, ... }:

{
  imports = [ ./nginx.nix ];

  services.nginx.virtualHosts."weblounge.org" = {
    default = true;
    serverAliases = [
      "www.weblounge.org"
      config.networking.fqdn
    ];
    enableACME = true;
    forceSSL = true;
    root = "${pkgs.weblounge.web}/landing";
    # Asset names carry no hash, so browsers must recheck them soon.
    extraConfig = "expires 1h;";
  };
}

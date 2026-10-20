{ config, lib, pkgs, ... }:

let
  inherit (config.weblounge) domain;
in
{
  imports = [ ./nginx.nix ];

  services.nginx.virtualHosts.${domain} = lib.mkMerge [
    (config.weblounge.nginx.errorPages { "404" = "404.html"; })
    {
      default = true;
      serverAliases = [
        "www.${domain}"
        config.networking.fqdn
      ];
      enableACME = true;
      forceSSL = true;
      root = "${pkgs.weblounge.web}/landing";
      # The page, its stylesheet and its script keep their names, so browsers
      # must recheck them soon.
      extraConfig = "expires 1h;";
      # What the stylesheet loads carries esbuild's content hash in its name.
      # An add_header here drops the server's, so the location repeats them.
      locations."~ \"^/assets/[^/]+-[A-Z2-7]{8}\\.[a-z]+$\"".extraConfig = ''
        expires off;
        add_header Cache-Control "public, max-age=31536000, immutable" always;
      '' + config.weblounge.nginx.securityHeaders { };
    }
  ];
}

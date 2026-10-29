{ config, lib, pkgs, ... }:

let
  inherit (config.weblounge) ergo;
  hostName = ergo.serverName;
  socket = ergo.websocketUrl hostName;
  members = config.weblounge.concierge.publicUrl;
  upload = "${config.weblounge.origin config.weblounge.files.hostName}/upload";

  gamjaConfig = pkgs.writeText "gamja-config.json" (builtins.toJSON {
    server = {
      url = socket;
      autojoin = "#lounge";
      auth = "oauth2";
      autoconnect = true;
    };
    oauth2 = {
      url = members;
      client_id = "gamja";
    };
  });
in
{
  imports = [ ./concierge.nix ./ergo.nix ./nginx.nix ];

  services.nginx.virtualHosts.${hostName} = lib.mkMerge [
    (config.weblounge.nginx.errorPages { "404" = "404.html"; })
    {
      globalRedirect = null;
      root = pkgs.weblounge.gamja;
      # gamja connects to Ergo's WebSocket, fetches the OAuth metadata and
      # token from concierge, and posts pasted images to the upload endpoint.
      extraConfig = config.weblounge.nginx.securityHeaders {
        connect-src = "'self' ${socket} ${members} ${upload}";
        manifest-src = "'self'";
      };
      locations."= /config.json".alias = gamjaConfig;
      locations."= /socket" = ergo.websocketProxy;
    }
  ];
}

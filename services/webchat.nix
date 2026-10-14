{ config, lib, pkgs, ... }:

let
  inherit (config.weblounge) ergo;
  hostName = ergo.serverName;
  origin = config.weblounge.origin hostName;
  socket = "${lib.replaceStrings [ "https://" ] [ "wss://" ] origin}/socket";
  members = config.weblounge.concierge.publicUrl;

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

  services.nginx.virtualHosts.${hostName} = {
    globalRedirect = null;
    root = pkgs.weblounge.gamja;
    # gamja connects to Ergo's WebSocket and fetches the OAuth metadata and
    # token from concierge.
    extraConfig = config.weblounge.nginx.securityHeaders {
      connect-src = "'self' ${socket} ${members}";
      manifest-src = "'self'";
    };
    locations."= /config.json".alias = gamjaConfig;
    locations."= /socket" = {
      proxyPass = "http://${ergo.websocketListener}";
      proxyWebsockets = true;
      # Ergo takes the client's address from X-Forwarded-For when nginx, on
      # loopback, sends it, so nginx replaces whatever the client sent.
      recommendedProxySettings = false;
      extraConfig = ''
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto https;
        proxy_read_timeout 1d;
        proxy_send_timeout 1d;
      '';
    };
  };
}

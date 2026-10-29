{ config, lib, pkgs, ... }:

let
  inherit (config.weblounge) ergo;
  hostName = "beta.${config.weblounge.domain}";
  origin = config.weblounge.origin hostName;
  socket = ergo.websocketUrl hostName;
  members = config.weblounge.concierge.publicUrl;
  upload = "${config.weblounge.origin config.weblounge.files.hostName}/upload";

  clientConfig = pkgs.writeText "weblounge-client-config.json" (builtins.toJSON {
    mode = "hosted";
    network = {
      name = ergo.settings.network.name;
      endpoint = socket;
      autojoin = ergo.settings.channels.auto-join;
    };
    oauth = {
      issuer = members;
      client_id = "weblounge-client";
      redirect_uri = "${origin}/";
    };
  });

  # The client's engine is wasm, compiled in a SharedWorker.
  policy = {
    script-src = "'self' 'wasm-unsafe-eval'";
    worker-src = "'self'";
    font-src = "'self'";
    connect-src = "'self' ${socket} ${members} ${upload}";
  };
  headersWith = cacheControl:
    config.weblounge.nginx.securityHeadersWith policy { Cache-Control = cacheControl; };
in
{
  imports = [ ./concierge.nix ./ergo.nix ./files.nix ./nginx.nix ];

  weblounge.ergo.settings.server.websockets.allowed-origins = [ origin ];

  systemd.services.concierge.serviceConfig.Environment = [ "CONCIERGE_BETA_CLIENT_URL=${origin}" ];

  services.nginx.virtualHosts.${hostName} = lib.mkMerge [
    (config.weblounge.nginx.errorPages { "404" = "404.html"; })
    {
      enableACME = true;
      forceSSL = true;
      root = pkgs.weblounge.client;
      # Only the files under /assets/ carry their content's hash in their
      # names; the rest keep theirs across builds. The error pages' assets
      # set their own Cache-Control, so the server level sets none.
      extraConfig = config.weblounge.nginx.securityHeaders policy;
      locations."/".extraConfig = headersWith "no-cache";
      locations."/assets/".extraConfig = headersWith "public, max-age=31536000, immutable";
      locations."= /config.json" = {
        alias = clientConfig;
        extraConfig = headersWith "no-cache";
      };
      locations."= /socket" = ergo.websocketProxy;
    }
  ];
}

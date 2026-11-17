# weblounge-client, the web client, served at each host name in
# weblounge.webClients with the build given there.
{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.webClients;
  inherit (config.weblounge) ergo pushRelay;
  members = config.weblounge.concierge.publicUrl;
  files = config.weblounge.origin config.weblounge.files.hostName;
  upload = "${files}/upload";
  origins = map config.weblounge.origin (lib.attrNames cfg);

  clientConfig = hostName: pkgs.writeText "weblounge-client-config.json" (builtins.toJSON {
    mode = "hosted";
    network = {
      name = ergo.settings.network.name;
      endpoint = ergo.websocketUrl hostName;
      autojoin = ergo.settings.channels.auto-join;
    };
    oauth = {
      issuer = members;
      client_id = "weblounge-client";
      redirect_uri = "${config.weblounge.origin hostName}/";
      session_token_endpoint = "${members}/oauth/session-token";
    };
    push = {
      relay = pushRelay.publicUrl;
      relay_vapid = pushRelay.vapidPublicKey;
    };
  });

  # The client's engine is wasm, compiled in a SharedWorker. The composer
  # previews a pasted image as a blob: URL, and messages show uploads inline.
  policy = hostName: {
    img-src = "'self' blob: ${files}";
    script-src = "'self' 'wasm-unsafe-eval'";
    worker-src = "'self'";
    font-src = "'self'";
    manifest-src = "'self'";
    connect-src = "'self' ${ergo.websocketUrl hostName} ${members} ${upload} ${pushRelay.publicUrl}";
  };

  virtualHost = hostName: client:
    let
      headersWith = cacheControl:
        config.weblounge.nginx.securityHeadersWith (policy hostName) { Cache-Control = cacheControl; };
    in
    lib.mkMerge [
      (config.weblounge.nginx.errorPages { "404" = "404.html"; })
      {
        enableACME = true;
        forceSSL = true;
        globalRedirect = null;
        root = client.package;
        # Only the files under /assets/ carry their content's hash in their
        # names; the rest keep theirs across builds. The error pages' assets
        # set their own Cache-Control, so the server level sets none.
        extraConfig = config.weblounge.nginx.securityHeaders (policy hostName);
        # The client routes by path, so a reload of /weblounge/lounge gets
        # index.html; a missing asset under /assets/ still answers 404.
        locations."/" = {
          tryFiles = "$uri /index.html";
          extraConfig = headersWith "no-cache";
        };
        locations."/assets/".extraConfig = headersWith "public, max-age=31536000, immutable";
        locations."= /config.json" = {
          alias = clientConfig hostName;
          extraConfig = headersWith "no-cache";
        };
        locations."= /socket" = ergo.websocketProxy;
      }
    ];
in
{
  imports = [ ./concierge.nix ./ergo.nix ./files.nix ./nginx.nix ./push-relay.nix ];

  options.weblounge.webClients = lib.mkOption {
    type = lib.types.attrsOf (lib.types.submodule {
      options.package = lib.mkOption {
        type = lib.types.package;
        description = "The build of weblounge-client to serve.";
      };
    });
    default = { };
    description = "The host names weblounge-client is served at, each with its build.";
  };

  config = {
    weblounge.ergo.settings.server.websockets.allowed-origins = origins;
    weblounge.pushRelay.allowedOrigins = origins;
    services.nginx.virtualHosts = lib.mapAttrs virtualHost cfg;
  };
}

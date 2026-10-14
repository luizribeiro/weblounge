{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  listenerSettings = {
    ergo-api = listener: { weblounge.ergo.settings.api.listener = lib.mkForce listener; };
    ergo-websocket = listener: { weblounge.ergo.websocketListener = lib.mkForce listener; };
    concierge = listener: { weblounge.concierge.listen = lib.mkForce listener; };
  };

  rejects = setListener: listener:
    let
      host = weblounge.hosts.credenza.extendModules { modules = [ (setListener listener) ]; };
    in
    lib.any (check: !check.assertion && lib.hasInfix "loopback" check.message)
      host.config.assertions;

  shouldReject = {
    "127.0.0.1:8089" = false;
    "[::1]:8089" = false;
    "localhost:8089" = false;
    ":8089" = true;
    "0.0.0.0:8089" = true;
    "[::]:8089" = true;
    "107.173.231.206:8089" = true;
    "127.0.0.1.example.com:8089" = true;
  };
in
lib.mapAttrs'
  (service: setListener: lib.nameValuePair "test-${service}-rejects-non-loopback-listeners" {
    expr = lib.mapAttrs (listener: _: rejects setListener listener) shouldReject;
    expected = shouldReject;
  })
  listenerSettings

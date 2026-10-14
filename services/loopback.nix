{ config, lib, ... }:

{
  options.weblounge.loopbackListeners = lib.mkOption {
    type = lib.types.attrsOf lib.types.str;
    default = { };
    description = "Listeners, as host:port and named for error messages, that must not be reachable from other machines.";
  };

  config.assertions = lib.mapAttrsToList
    (name: listener: {
      assertion = builtins.match "(127\\.0\\.0\\.1|\\[::1]|localhost):[0-9]+" listener != null;
      message = "${name} must listen on loopback only, not on ${listener}.";
    })
    config.weblounge.loopbackListeners;
}

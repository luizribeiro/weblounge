# The push relay at push.weblounge.org, from weblounge-client: homes such
# as our Ergo POST members' encrypted pushes to it, and it passes them on
# to the browsers' push services, which it signs for with its VAPID key.
{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.pushRelay;
  hostName = "push.${config.weblounge.domain}";
  portOf = listener: lib.toInt (lib.last (lib.splitString ":" listener));
  stateDirectory = "push-relay";
in
{
  imports = [ ./concierge.nix ./egress.nix ./loopback.nix ./sandbox.nix ./secrets.nix ./sqlite-vacuum.nix ];

  options.weblounge.pushRelay.listen = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8098";
    description = "Where the push relay listens for nginx, as host:port.";
  };

  config = {
    weblounge.loopbackListeners."The push relay" = cfg.listen;

    # The key the relay signs its pushes to the push services with, as PEM;
    # its public half is in the web client's config.json.
    weblounge.secrets.push-relay-vapid-key = {
      fromSops = true;
      loadedBy = [ "push-relay" ];
    };
    weblounge.secrets.push-relay-introspection-secret.loadedBy = [ "push-relay" ];

    weblounge.sqliteVacuum.push-relay = "/var/lib/${stateDirectory}/push-relay.sqlite3";

    # The push services are on 443; concierge checks members' session tokens.
    weblounge.egressPorts.push-relay = [ 443 ];
    weblounge.egressLoopbackPorts.push-relay = [ (portOf config.weblounge.concierge.listen) ];

    systemd.services.push-relay = {
      description = "The push relay, push.weblounge.org";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment = {
        PUSH_RELAY_LISTEN = cfg.listen;
        PUSH_RELAY_STATE_DIR = "/var/lib/${stateDirectory}";
        PUSH_RELAY_ORIGIN = config.weblounge.origin hostName;
        PUSH_RELAY_VAPID_KEY_FILE = "%d/push-relay-vapid-key";
        PUSH_RELAY_CONCIERGE_URL = config.weblounge.concierge.introspectionUrl;
        PUSH_RELAY_CONCIERGE_SECRET_FILE = "%d/push-relay-introspection-secret";
        TOKIO_WORKER_THREADS = "2";
      };
      serviceConfig = config.weblounge.sandbox // config.weblounge.internetOnly // {
        ExecStart = lib.getExe' pkgs.weblounge.push-relay "weblounge-push-relay";
        Restart = "on-failure";
        User = "push-relay";
        DynamicUser = true;
        StateDirectory = stateDirectory;
        StateDirectoryMode = "0700";

        # Besides the push services, it talks to concierge, and nginx to it.
        IPAddressAllow = config.weblounge.internetOnly.IPAddressAllow ++ [ "127.0.0.1" ];
        SocketBindAllow = "tcp:${toString (portOf cfg.listen)}";
        SocketBindDeny = "any";

        # A connection to each push service and nginx's, with room to spare.
        MemoryMax = "128M";
        TasksMax = 32;
        LimitNOFILE = 256;
      };
    };

    weblounge.deployChecks."the push relay's health check answers on loopback".script = ''
      ${pkgs.curl}/bin/curl --silent --show-error --fail --max-time 10 http://${cfg.listen}/healthz
    '';
  };
}

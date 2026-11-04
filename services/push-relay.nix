# The push relay at push.weblounge.org, from weblounge-client: homes such
# as our Ergo POST members' encrypted pushes to it, and it passes them on
# to the browsers' push services, which it signs for with its VAPID key.
{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.pushRelay;
  hostName = "push.${config.weblounge.domain}";
  portOf = listener: lib.toInt (lib.last (lib.splitString ":" listener));
  stateDirectory = "push-relay";

  proxy = {
    proxyPass = "http://${cfg.listen}";
    # The relay reads no client address, so nginx passes none.
    recommendedProxySettings = false;
  };
in
{
  imports = [ ./concierge.nix ./egress.nix ./loopback.nix ./nginx.nix ./sandbox.nix ./secrets.nix ./sqlite-vacuum.nix ];

  options.weblounge.pushRelay.listen = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8098";
    description = "Where the push relay listens for nginx, as host:port.";
  };

  options.weblounge.pushRelay.publicUrl = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = config.weblounge.origin hostName;
    description = "The push relay's origin, which web clients register devices with.";
  };

  # Derived from the push-relay-vapid-key secret; browsers check pushes against it.
  options.weblounge.pushRelay.vapidPublicKey = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = "BE-uu3eaHi7ikKStoHi-Gv2XQYPiIprG4idoy0-U2hAyjsk9dQlULNvwJnv90HCm1nZJStmf72-fmnUCNmaBUng";
    description = "The relay's VAPID public key, as unpadded base64url.";
  };

  options.weblounge.pushRelay.allowedOrigins = lib.mkOption {
    type = lib.types.listOf lib.types.str;
    default = [ ];
    description = "The web clients' origins, which the relay lets register devices and routes.";
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
        PUSH_RELAY_ORIGIN = cfg.publicUrl;
        PUSH_RELAY_ALLOWED_ORIGINS = lib.concatStringsSep "," cfg.allowedOrigins;
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

    # Homes are held per route, and each but this host per address too: our
    # Ergo pushes for every member from its public address. Devices are
    # held per address.
    services.nginx.appendHttpConfig = ''
      geo $push_relay_from_here {
        default 0;
        127.0.0.1 1;
        ${config.weblounge.publicIPv4} 1;
      }
      map $push_relay_from_here $push_relay_home {
        0 $binary_remote_addr;
        1 "";
      }
      limit_req_zone $uri zone=push_relay_routes:4m rate=5r/s;
      limit_req_zone $push_relay_home zone=push_relay_homes:2m rate=20r/s;
      limit_req_zone $binary_remote_addr zone=push_relay_devices:2m rate=1r/s;
    '';

    services.nginx.virtualHosts.${hostName} = {
      enableACME = true;
      forceSSL = true;
      # A home's push is under 3 KB, and the device API's bodies smaller.
      extraConfig = ''
        client_max_body_size 4k;
        limit_req_status 429;
      '';
      locations."/v1/r/" = proxy // {
        extraConfig = ''
          limit_req zone=push_relay_routes burst=100 nodelay;
          limit_req zone=push_relay_homes burst=200 nodelay;
        '';
      };
      locations."/" = proxy // {
        extraConfig = "limit_req zone=push_relay_devices burst=20 nodelay;";
      };
    };

    weblounge.deployChecks."the push relay's health check answers on loopback".script = ''
      ${pkgs.curl}/bin/curl --silent --show-error --fail --max-time 10 http://${cfg.listen}/healthz
    '';
  };
}

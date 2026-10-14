{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.concierge;
  hostName = "members.${config.weblounge.domain}";
  publicUrl = config.weblounge.origin hostName;
  concierge = lib.getExe pkgs.weblounge.concierge;

  # concierge-admin runs with the rest of the unit's serviceConfig, so admin
  # commands see exactly the identity, state and secrets the service does.
  serviceOnly = {
    ExecStart = "${concierge} serve";
    Restart = "on-failure";
  };

  sharedProperties = removeAttrs config.systemd.services.concierge.serviceConfig
    (lib.attrNames serviceOnly);

  systemdValue = value: if lib.isBool value then lib.boolToYesNo value else toString value;

  propertyFlags = lib.concatLists (lib.mapAttrsToList
    (name: values: map (value: "--property=${name}=${systemdValue value}") (lib.toList values))
    sharedProperties);

  admin = pkgs.writeShellApplication {
    name = "concierge-admin";
    text = ''
      exec ${config.security.wrapperDir}/sudo ${config.systemd.package}/bin/systemd-run \
        --quiet --pipe --wait --collect --service-type=exec \
        ${lib.escapeShellArgs propertyFlags} \
        ${concierge} "$@"
    '';
  };
in
{
  imports = [ ./ergo.nix ./loopback.nix ./nginx.nix ./secrets.nix ];

  options.weblounge.concierge.listen = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8080";
    description = "Where concierge listens for nginx, as host:port.";
  };

  config = {
    weblounge.loopbackListeners.Concierge = cfg.listen;

    weblounge.secrets.session-key.loadedBy = [ "concierge" ];
    weblounge.secrets.ergo-api-token.loadedBy = [ "concierge" ];

    systemd.services.concierge = {
      description = "Concierge, the members' site";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = serviceOnly // {
        User = "concierge";
        DynamicUser = true;
        StateDirectory = "concierge";
        StateDirectoryMode = "0700";
        UMask = "0077";
        Environment = [
          "CONCIERGE_LISTEN=${cfg.listen}"
          "CONCIERGE_PUBLIC_URL=${publicUrl}"
          "CONCIERGE_IRC_SERVER=${config.weblounge.ergo.address}"
          "CONCIERGE_ERGO_API=http://${config.weblounge.ergo.settings.api.listener}"
          "CONCIERGE_ASSETS=${pkgs.weblounge.web}/members"
        ];

        CapabilityBoundingSet = "";
        IPAddressAllow = "localhost";
        IPAddressDeny = "any";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        PrivateUsers = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
      };
    };

    environment.systemPackages = [ admin ];

    services.nginx.virtualHosts.${hostName} = {
      enableACME = true;
      forceSSL = true;
      locations."/" = {
        proxyPass = "http://${cfg.listen}";
        # Concierge trusts X-Real-IP for its sign-in limits, so nginx sets it
        # from the connection, replacing anything the client sent. The
        # recommended settings would add X-Forwarded-For too, which nothing
        # reads.
        recommendedProxySettings = false;
        extraConfig = ''
          proxy_set_header X-Real-IP $remote_addr;
        '';
      };
    };
  };
}

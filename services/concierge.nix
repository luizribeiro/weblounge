{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.concierge;
  inherit (config.weblounge) domain httpsPort;
  hostName = "members.${domain}";
  publicUrl = "https://${hostName}" + lib.optionalString (httpsPort != 443) ":${toString httpsPort}";
in
{
  imports = [ ./loopback.nix ./nginx.nix ./secrets.nix ];

  options.weblounge.concierge.listen = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8080";
    description = "Where concierge listens for nginx, as host:port.";
  };

  config = {
    weblounge.loopbackListeners.Concierge = cfg.listen;

    weblounge.secrets.session-key.loadedBy = [ "concierge" ];

    systemd.services.concierge = {
      description = "Concierge, the members' site";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${lib.getExe pkgs.weblounge.concierge} serve";
        Restart = "on-failure";

        User = "concierge";
        DynamicUser = true;
        StateDirectory = "concierge";
        StateDirectoryMode = "0700";
        UMask = "0077";
        Environment = [
          "CONCIERGE_LISTEN=${cfg.listen}"
          "CONCIERGE_PUBLIC_URL=${publicUrl}"
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

    services.nginx.virtualHosts.${hostName} = {
      enableACME = true;
      forceSSL = true;
      locations."/".proxyPass = "http://${cfg.listen}";
    };
  };
}

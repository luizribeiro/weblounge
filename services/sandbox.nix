{ config, lib, ... }:

{
  options.weblounge.sandbox = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default = {
      CapabilityBoundingSet = "";
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
      UMask = "0077";
    };
    description = "serviceConfig that confines an unprivileged service to its own files and Internet sockets.";
  };

  options.weblounge.runProperties = lib.mkOption {
    type = lib.types.functionTo (lib.types.listOf lib.types.str);
    readOnly = true;
    default =
      let
        systemdValue = value: if lib.isBool value then lib.boolToYesNo value else toString value;
      in
      serviceConfig: lib.concatLists (lib.mapAttrsToList
        (name: values: map (value: "--property=${name}=${systemdValue value}") (lib.toList values))
        serviceConfig);
    description = "systemd-run's flags for running a command with a unit's serviceConfig.";
  };

  options.weblounge.loopbackOnly = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default = {
      IPAddressAllow = "localhost";
      IPAddressDeny = "any";
    };
    description = "serviceConfig for a service that only talks to others on this machine.";
  };

  options.weblounge.internetOnly = lib.mkOption {
    type = lib.types.attrsOf lib.types.anything;
    readOnly = true;
    default = {
      IPAddressDeny = [ "localhost" "link-local" "multicast" "10.0.0.0/8" "100.64.0.0/10" "172.16.0.0/12" "192.168.0.0/16" "fc00::/7" ];
      IPAddressAllow = [ "127.0.0.53" ];

      # The trust store in /etc and resolved's address are all it reads
      # beyond the store and what it binds.
      TemporaryFileSystem = [ "/var:ro" "/run:ro" ];
      BindReadOnlyPaths = [ "/run/systemd/resolve" ];
      # PrivateTmp= and PrivateDevices= mount their own /tmp and /dev/shm.
      NoExecPaths = [ "/" "/tmp" "/dev/shm" ];
      ExecPaths = [ "/nix/store" ];
      PrivateIPC = true;

      SystemCallFilter = config.weblounge.sandbox.SystemCallFilter
        ++ [ "~@chown" "~@ipc" "~@keyring" "~@memlock" "~@resources" "~@setuid" ];
      SystemCallErrorNumber = "EPERM";
    };
    description = "serviceConfig, over weblounge.sandbox, for a service that talks to servers on the Internet: nothing on this machine but resolved's stub, nothing on private networks, and no files but the store, /etc and what it binds.";
  };
}

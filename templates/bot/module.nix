{ config, lib, pkgs, ... }:

let
  inherit (lib) mkOption types;

  enabled = lib.filterAttrs (_: bot: bot.enable) config.services.weblounge-bot;

  botOptions = { name, ... }: {
    options = {
      enable = lib.mkEnableOption "the IRC bot ${name}";

      package = mkOption {
        type = types.package;
        default = pkgs.callPackage ./package.nix { };
        defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
        description = "The bot to run.";
      };

      nick = mkOption {
        type = types.str;
        default = name;
        description = "The bot's account name, which is also its nick.";
      };

      channels = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "#lounge" ];
        description = "Channels the bot joins once connected.";
      };

      server = mkOption {
        type = types.str;
        default = "irc.weblounge.org:6697";
        description = "The IRC server, as host:port, spoken to over TLS.";
      };

      passwordFile = mkOption {
        type = types.str;
        example = "/run/secrets/bender-password";
        description = ''
          A file holding the account's password. systemd reads it as root and
          hands it to the bot as a credential. A string rather than a path, so
          the password never lands in the world-readable Nix store.
        '';
      };

      caFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = ''
          A CA to trust besides the system's, such as a test server's
          self-signed one. Read by systemd, so it may be readable by root only.
        '';
      };
    };
  };

  service = name: bot: lib.nameValuePair "weblounge-bot-${name}" {
    description = "IRC bot ${name}";
    wantedBy = [ "multi-user.target" ];
    wants = [ "network-online.target" ];
    after = [ "network-online.target" ];

    environment = {
      IRC_SERVER = bot.server;
      IRC_NICK = bot.nick;
      IRC_CHANNELS = lib.concatStringsSep "," bot.channels;
    } // lib.optionalAttrs (bot.caFile != null) {
      IRC_CA_FILE = "%d/ca";
    };

    serviceConfig = {
      ExecStart = lib.getExe bot.package;
      LoadCredential = [ "password:${bot.passwordFile}" ]
        ++ lib.optional (bot.caFile != null) "ca:${bot.caFile}";
      Restart = "on-failure";
      RestartSec = 10;

      DynamicUser = true;
      UMask = "0077";
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
      # No AF_UNIX: the bot is built without cgo, so Go resolves names
      # itself over DNS rather than asking systemd-resolved through NSS.
      RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
      RestrictNamespaces = true;
      RestrictRealtime = true;
      RestrictSUIDSGID = true;
      SystemCallArchitectures = "native";
      SystemCallFilter = [ "@system-service" "~@privileged" ];
    };
  };
in
{
  options.services.weblounge-bot = mkOption {
    type = types.attrsOf (types.submodule botOptions);
    default = { };
    description = "IRC bots on weblounge.org, each with an account of its own.";
  };

  config = {
    assertions = lib.mapAttrsToList
      (name: bot: {
        assertion = !lib.hasPrefix builtins.storeDir bot.passwordFile;
        message = "services.weblounge-bot.${name}.passwordFile is in the Nix store, which anyone on the machine can read.";
      })
      enabled;

    systemd.services = lib.mapAttrs' service enabled;
  };
}

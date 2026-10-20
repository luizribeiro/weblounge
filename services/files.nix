{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.files;
  scrubber = lib.getExe pkgs.weblounge.scrubber;
in
{
  imports = [ ./sandbox.nix ];

  options.weblounge.files = {
    hostName = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = "files.${config.weblounge.domain}";
      description = "The host uploads are served from, and taken on.";
    };
    directory = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = "/var/lib/concierge-files";
      description = "Where uploads are stored, scrubbed, under their ids.";
    };
    group = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = "concierge-files";
      description = "The group that reads the uploads; the scrubber and concierge also write them.";
    };
    scrubber = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      readOnly = true;
      default = {
        socket = "/run/concierge-scrubber.sock";
        group = "concierge-scrubber-socket";
      };
      description = "The socket concierge hands uploads to the scrubber on, and the group that may.";
    };
  };

  config = {
    users.groups.${cfg.group} = { };
    users.groups.${cfg.scrubber.group} = { };

    # Files in it take its group.
    systemd.tmpfiles.settings.concierge-files.${cfg.directory}.d = {
      mode = "2770";
      user = "root";
      inherit (cfg) group;
    };

    # Each upload gets a scrubber of its own, so one that a crafted image
    # takes over sees no other upload's bytes.
    systemd.sockets.concierge-scrubber = {
      description = "Where concierge hands uploads to the scrubber";
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = cfg.scrubber.socket;
        Accept = true;
        SocketMode = "0660";
        SocketGroup = cfg.scrubber.group;
        MaxConnections = 16;
      };
    };

    systemd.services."concierge-scrubber@" = {
      description = "Scrub an upload of its metadata and store it";
      environment.CONCIERGE_UPLOADS = cfg.directory;
      serviceConfig = config.weblounge.sandbox // {
        ExecStart = scrubber;
        StandardInput = "socket";
        StandardOutput = "socket";
        StandardError = "journal";
        User = "concierge-scrubber";
        DynamicUser = true;
        SupplementaryGroups = [ cfg.group ];

        # It talks to nobody: concierge's connection comes in as its input.
        PrivateNetwork = true;
        RestrictAddressFamilies = "none";
        IPAddressDeny = "any";

        TemporaryFileSystem = [ "/var:ro" "/run:ro" ];
        BindPaths = [ cfg.directory ];
        # PrivateTmp= and PrivateDevices= mount their own /tmp and /dev/shm.
        NoExecPaths = [ "/" "/tmp" "/dev/shm" ];
        ExecPaths = [ "/nix/store" ];
        PrivateIPC = true;

        SystemCallFilter = config.weblounge.sandbox.SystemCallFilter
          ++ [ "~@chown" "~@ipc" "~@keyring" "~@memlock" "~@resources" "~@setuid" ];
        SystemCallErrorNumber = "EPERM";

        # An upload of the most allowed, a copy of it and what's kept of it.
        MemoryMax = "64M";
        TasksMax = 4;
        LimitNOFILE = 16;
        # A client sending its upload slowly holds its scrubber; one that
        # never finishes doesn't hold it forever.
        RuntimeMaxSec = "5min";
      };
    };
  };
}

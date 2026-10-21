{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.files;
  scrubber = lib.getExe pkgs.weblounge.scrubber;

  # Uploads are images served as they are, which nothing should run,
  # frame, index or follow links out of. The rest of the default policy is
  # for pages, which this site has none of.
  headers = config.weblounge.nginx.securityHeadersWith
    {
      script-src = null;
      style-src = null;
      img-src = null;
      form-action = null;
      base-uri = null;
      sandbox = "";
    }
    {
      Referrer-Policy = "no-referrer";
      X-Robots-Tag = "noindex, nofollow, noimageindex, noai, noimageai";
    };
  # The scrubber writes each upload under a dotted name until it's whole.
  uploadPath = "^/[^/.][^/]*$";
in
{
  imports = [ ./nginx.nix ./sandbox.nix ];

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
    users.users.nginx.extraGroups = [ cfg.group ];
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

    # IRC clients find the upload endpoint in ISUPPORT: gamja and senpai read
    # soju.im/FILEHOST, the draft spec draft/FILEHOST.
    weblounge.ergo.settings.server.additional-isupport =
      lib.genAttrs [ "soju.im/FILEHOST" "draft/FILEHOST" ] (_: "${config.weblounge.origin cfg.hostName}/upload");

    services.nginx.virtualHosts.${cfg.hostName} = {
      enableACME = true;
      forceSSL = true;
      # An upload's link is all that keeps it private, so none is logged.
      extraConfig = ''
        access_log off;
      '' + headers;
      locations."~ \"${uploadPath}\"" = {
        root = cfg.directory;
        # nginx serves each type by its extension; browsers show these
        # inline, and are told so.
        extraConfig = headers + ''
          default_type application/octet-stream;
          add_header Cross-Origin-Resource-Policy "cross-origin" always;
          add_header Content-Disposition "inline" always;
        '';
      };
      locations."= /robots.txt" = {
        return = "200 \"User-agent: *\\nDisallow: /\\n\"";
        extraConfig = "default_type text/plain;";
      };
      # Concierge takes uploads, in a stream, and holds them to their limit
      # itself, answering 413 with Upload-Limit; nginx stops only what's far
      # past it.
      locations."= /upload" = {
        proxyPass = "http://${config.weblounge.concierge.listen}";
        recommendedProxySettings = false;
        extraConfig = ''
          client_max_body_size 16m;
          proxy_request_buffering off;
          proxy_http_version 1.1;
          proxy_set_header X-Real-IP $remote_addr;
        '';
      };
      locations."/".return = "404";
    };
  };
}

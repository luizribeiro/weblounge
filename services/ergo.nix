{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.ergo;
  yaml = pkgs.formats.yaml { };
  inherit (config.weblounge) domain;
  serverName = "irc.${domain}";
  port = 6697;
  credentials = "/run/credentials/ergochat.service";
  oauth2 = cfg.settings.accounts.oauth2.enabled or false;

in
{
  imports = [ ./loopback.nix ./nginx.nix ./secrets.nix ];

  # The nixpkgs module's defaults leave out fakelag, login throttling, nick
  # reservation, cloaking and logging; Ergo's default.yaml keeps them on.
  options.weblounge.ergo.settings = lib.mkOption {
    inherit (yaml) type;
    default = { };
    description = "Ergo settings, merged over the default.yaml that Ergo ships.";
  };

  options.weblounge.ergo.serverName = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = serverName;
    description = "The IRC server's name, which is also the web client's host.";
  };

  options.weblounge.ergo.websocketListener = lib.mkOption {
    type = lib.types.str;
    default = "127.0.0.1:8097";
    description = "Where Ergo takes WebSocket connections from nginx, as host:port, without TLS.";
  };

  options.weblounge.ergo.localAddress = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = "${serverName}:${toString port}";
    description = "Where clients on this machine reach Ergo over TLS, as host:port; the name resolves to loopback here.";
  };

  options.weblounge.ergo.address = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = "${serverName}:${toString config.weblounge.ircPort}";
    description = "Where IRC clients connect, over TLS, as host:port.";
  };

  config = {
    weblounge.loopbackListeners."Ergo's API" = cfg.settings.api.listener;
    weblounge.loopbackListeners."Ergo's WebSocket listener" = cfg.websocketListener;

    weblounge.ergo.settings = {
      network.name = "weblounge";
      server = {
        name = serverName;
        listeners.":${toString port}" = {
          tls = {
            cert = "${credentials}/fullchain.pem";
            key = "${credentials}/key.pem";
          };
          min-tls-version = "1.2";
        };
        # nginx terminates TLS and passes the client's address in
        # X-Forwarded-For, which Ergo trusts from proxy-allowed-from's
        # default, localhost.
        listeners.${cfg.websocketListener}.websocket = true;
        websockets.allowed-origins = [ (config.weblounge.origin serverName) ];
        motd = "${./ergo.motd}";
        ip-cloaking.netname = "weblounge";
      };
      accounts = {
        registration = {
          enabled = false;
          # Members choose their own passwords; Ergo's default is 4.
          bcrypt-cost = 12;
        };
        require-sasl = {
          enabled = true;
          exempted = [ ];
        };
        multiclient = {
          always-on = "opt-out";
          auto-away = "opt-out";
        };
      };
      channels.auto-join = [ "#lounge" ];
      history = {
        persistent = {
          enabled = true;
          unregistered-channels = true;
          direct-messages = "disabled";
        };
        retention.allow-individual-delete = true;
      };
      datastore.sqlite.enabled = true;
      webpush = {
        enabled = true;
        subscriber = "https://${domain}";
      };
      api = {
        enabled = true;
        listener = "127.0.0.1:8089";
      };
      # nixpkgs' ergo ships no translations for the default path to find.
      languages.enabled = false;
      # connect-ip lines carry client addresses, which we promise not to keep.
      logging = [{
        method = "stderr";
        type = "* -userinput -useroutput -connect-ip";
        level = "info";
      }];
    };

    services.ergochat = {
      enable = true;
      configFile = pkgs.weblounge.ergoConfig cfg.settings;
    };

    weblounge.secrets.ergo-api-token.loadedBy = [ "ergochat" ];
    weblounge.secrets.oauth-introspection-secret.loadedBy = lib.mkIf oauth2 [ "ergochat" ];

    systemd.services.ergochat = {
      wants = [ "acme-${serverName}.service" ];
      after = [ "acme-${serverName}.service" ];
      serviceConfig = {
        # Ergo reads its secrets only from its config file or environment.
        ExecStart = lib.mkForce (pkgs.writeShellScript "ergo-start" (''
          export ERGO__API__BEARER_TOKENS="[\"$(< "$CREDENTIALS_DIRECTORY/ergo-api-token")\"]"
        '' + lib.optionalString oauth2 ''
          export ERGO__ACCOUNTS__OAUTH2__CLIENT_SECRET="$(< "$CREDENTIALS_DIRECTORY/oauth-introspection-secret")"
        '' + ''
          exec ${lib.getExe pkgs.ergochat} run --conf /etc/ergo.yaml
        ''));
        SyslogIdentifier = "ergo";
        # Ergo's RELOADING=1 carries no MONOTONIC_USEC, which notify-reload
        # waits for until it times out.
        Type = lib.mkForce "notify";
        ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
        # default.yaml keeps the database and lock file in the working directory.
        WorkingDirectory = "/var/lib/ergo";
        LoadCredential = map (file: "${file}:/var/lib/acme/${serverName}/${file}") [ "fullchain.pem" "key.pem" ];
        RefreshOnReload = "credentials";
      };
    };

    # Ergo deletes expired history rows, but SQLite leaves their bytes in
    # free pages and the write-ahead log; rewriting both clears them out.
    systemd.services.ergo-history-vacuum = {
      description = "Clear expired chat history out of Ergo's database files";
      startAt = "hourly";
      serviceConfig = {
        Type = "oneshot";
        # The same dynamic user as Ergo's, which owns the database.
        User = "ergochat";
        DynamicUser = true;
        StateDirectory = "ergo";
        WorkingDirectory = "/var/lib/ergo";
        ExecStart = pkgs.writeShellScript "ergo-history-vacuum" ''
          checkpoint=$(${lib.getExe pkgs.sqlite} -cmd '.timeout 10000' ergo_history.db \
            'VACUUM' 'PRAGMA wal_checkpoint(TRUNCATE)')
          [[ $checkpoint == 0\|* ]] || { echo "the write-ahead log was busy: $checkpoint"; exit 1; }
        '';
      };
    };

    security.acme.certs.${serverName}.reloadServices = [ "ergochat.service" ];

    services.nginx.virtualHosts.${serverName} = {
      enableACME = true;
      forceSSL = true;
      globalRedirect = lib.mkDefault domain;
    };

    networking.hosts."127.0.0.1" = [ serverName ];

    networking.firewall.allowedTCPPorts = [ port ];
  };
}

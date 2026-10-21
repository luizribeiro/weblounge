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
  imports = [ ./loopback.nix ./nginx.nix ./sandbox.nix ./secrets.nix ./sqlite-vacuum.nix ];

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
        # Any bits of the address in a cloak let whoever gets ircd.db, which
        # holds the key, try every IPv4 address until one matches.
        ip-cloaking = {
          netname = "weblounge";
          num-bits = 0;
        };
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
        # Concierge keeps the records of accounts and bots' vhosts, so
        # changes to them go through it. NS CERT stays: the certificate
        # guide on /clients relies on CERT ADD.
        disabled-service-commands = {
          nickserv = [ "unregister" "passwd" ];
          hostserv = [ "on" "off" ];
        };
      };
      channels.auto-join = [ "#lounge" ];
      history = {
        persistent = {
          enabled = true;
          unregistered-channels = true;
          direct-messages = "opt-out";
        };
        restrictions.expire-time = "2w";
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

    # Ergo and concierge both read the token as they start, so a fresh one
    # each boot costs nothing.
    weblounge.secrets.ergo-api-token = {
      ephemeral = true;
      loadedBy = [ "ergochat" ];
    };
    weblounge.secrets.oauth-introspection-secret.loadedBy = lib.mkIf oauth2 [ "ergochat" ];

    systemd.services.ergochat = {
      wants = [ "acme-${serverName}.service" ];
      after = [ "acme-${serverName}.service" ];
      # Web push reaches whatever servers members' browsers name, so ergo
      # keeps the Internet; its own dialer refuses internal addresses.
      serviceConfig = config.weblounge.sandbox // {
        # sd_notify, and name lookups for web push: ergo is built with cgo,
        # so glibc resolves names, through nscd.
        RestrictAddressFamilies = config.weblounge.sandbox.RestrictAddressFamilies ++ [ "AF_UNIX" ];
        StateDirectoryMode = "0700";
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

    weblounge.sqliteVacuum.ergochat = "/var/lib/ergo/ergo_history.db";

    security.acme.certs.${serverName}.reloadServices = [ "ergochat.service" ];

    services.nginx.virtualHosts.${serverName} = {
      enableACME = true;
      forceSSL = true;
      globalRedirect = lib.mkDefault domain;
    };

    networking.hosts."127.0.0.1" = [ serverName ];

    networking.firewall.allowedTCPPorts = [ port ];

    # What concierge relies on, asked the way concierge asks. Only reading
    # the token takes root. The header goes in on standard input, since any
    # user on the machine can read a command line.
    weblounge.deployChecks."Ergo's API answers its token".script = ''
      token=$(${config.security.wrapperDir}/sudo cat ${config.weblounge.secrets.ergo-api-token.path})
      printf 'Authorization: Bearer %s\n' "$token" \
        | ${pkgs.curl}/bin/curl --silent --show-error --max-time 10 -H @- -d '{}' \
          http://${cfg.settings.api.listener}/v1/status | ${pkgs.gnugrep}/bin/grep '"success":true'
    '';

    weblounge.deployChecks."IRC turns away clients that don't log in" = {
      runs = "outside";
      script = ''
        reply=$(printf 'NICK deploy-check\r\nUSER deploy-check 0 * :deploy check\r\n' \
          | ${pkgs.coreutils}/bin/timeout 10 ${pkgs.openssl}/bin/openssl s_client -quiet -connect ${cfg.address} \
            -servername ${serverName} ${lib.optionalString (config.weblounge.selfSignedCa == null) "-verify_return_error"} 2>&1)
        ${pkgs.gnugrep}/bin/grep -q '^ERROR .*SASL' <<<"$reply" || { echo "$reply"; exit 1; }
      '';
    };
  };
}

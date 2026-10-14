{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.ergo;
  yaml = pkgs.formats.yaml { };
  inherit (config.weblounge) domain;
  serverName = "irc.${domain}";
  port = 6697;
  credentials = "/run/credentials/ergochat.service";

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

  options.weblounge.ergo.address = lib.mkOption {
    type = lib.types.str;
    readOnly = true;
    default = "${serverName}:${toString port}";
    description = "Where IRC clients connect, over TLS, as host:port.";
  };

  config = {
    weblounge.loopbackListeners."Ergo's API" = cfg.settings.api.listener;

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
        motd = "${./ergo.motd}";
        ip-cloaking.netname = "weblounge";
      };
      accounts = {
        registration.enabled = false;
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
      history.retention.allow-individual-delete = true;
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

    systemd.services.ergochat = {
      wants = [ "acme-${serverName}.service" ];
      after = [ "acme-${serverName}.service" ];
      serviceConfig = {
        # Ergo reads its API token only from its config file or environment.
        ExecStart = lib.mkForce (pkgs.writeShellScript "ergo-start" ''
          export ERGO__API__BEARER_TOKENS="[\"$(< "$CREDENTIALS_DIRECTORY/ergo-api-token")\"]"
          exec ${lib.getExe pkgs.ergochat} run --conf /etc/ergo.yaml
        '');
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

    security.acme.certs.${serverName}.reloadServices = [ "ergochat.service" ];

    services.nginx.virtualHosts.${serverName} = {
      enableACME = true;
      forceSSL = true;
      globalRedirect = domain;
    };

    networking.firewall.allowedTCPPorts = [ port ];
  };
}

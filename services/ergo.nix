{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.ergo;
  yaml = pkgs.formats.yaml { };
  serverName = "irc.weblounge.org";
  credentials = "/run/credentials/ergochat.service";

  # default.yaml's listeners, WebIRC gateway and admin oper are live
  # examples, with published credentials, rather than defaults.
  configFile = pkgs.runCommand "ergo.yaml" { nativeBuildInputs = [ pkgs.yq-go ]; } ''
    yq eval-all '
      (select(fi == 0) | del(.server.listeners, .server.webirc, .opers))
        * select(fi == 1)
      | ... comments = ""
    ' ${pkgs.ergochat.src}/default.yaml ${yaml.generate "ergo-settings.yaml" cfg.settings} > $out
  '';
in
{
  imports = [ ./nginx.nix ./secrets.nix ];

  # The nixpkgs module's defaults leave out fakelag, login throttling, nick
  # reservation, cloaking and logging; Ergo's default.yaml keeps them on.
  options.weblounge.ergo.settings = lib.mkOption {
    inherit (yaml) type;
    default = { };
    description = "Ergo settings, merged over the default.yaml that Ergo ships.";
  };

  config = {
    assertions = [{
      assertion = builtins.match "(127\\.0\\.0\\.1|\\[::1]|localhost):[0-9]+" cfg.settings.api.listener != null;
      message = "Ergo's API must listen on loopback only, not on ${cfg.settings.api.listener}.";
    }];

    weblounge.ergo.settings = {
      network.name = "weblounge";
      server = {
        name = serverName;
        listeners.":6697" = {
          tls = {
            cert = "${credentials}/fullchain.pem";
            key = "${credentials}/key.pem";
          };
          min-tls-version = "1.2";
        };
      };
      accounts = {
        registration.enabled = false;
        require-sasl = {
          enabled = true;
          exempted = [ ];
        };
      };
      api = {
        enabled = true;
        listener = "127.0.0.1:8089";
      };
      # nixpkgs' ergo ships no translations for the default path to find.
      languages.enabled = false;
    };

    services.ergochat = {
      enable = true;
      inherit configFile;
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
      globalRedirect = config.networking.domain;
    };

    networking.firewall.allowedTCPPorts = [ 6697 ];
  };
}

{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.mail;
  inherit (config.weblounge) domain;
  sender = "noreply@${domain}";
  inherit (config.weblounge.concierge) outbox;
  mailer = config.systemd.services.concierge-mailer;

  serviceOnly = {
    ExecStart = "${lib.getExe pkgs.weblounge.concierge} mailer";
    Restart = "on-failure";
  };

  # `concierge mailer --check`, confined, configured and credentialed as
  # the mailer is, so it fails wherever the mailer would.
  check = pkgs.writeShellScript "concierge-mailer-check" ''
    exec ${config.security.wrapperDir}/sudo ${config.systemd.package}/bin/systemd-run \
      --quiet --pipe --wait --collect --service-type=exec \
      ${lib.escapeShellArgs (config.weblounge.runProperties (removeAttrs mailer.serviceConfig (lib.attrNames serviceOnly))
        ++ lib.mapAttrsToList (name: value: "--setenv=${name}=${value}") mailer.environment)} \
      ${serviceOnly.ExecStart} --check
  '';
in
{
  imports = [ ./concierge.nix ./egress.nix ./sandbox.nix ./secrets.nix ];

  options.weblounge.mail.server = lib.mkOption {
    type = lib.types.str;
    default = "shadow.mxrouting.net";
    description = "The SMTP server mail is sent through, over implicit TLS on 465, as the sender's mailbox.";
  };

  config = {
    # The password of the noreply@ mailbox, which mail is sent from.
    weblounge.secrets.smtp-password = {
      fromSops = true;
      loadedBy = [ "concierge-mailer" ];
    };

    weblounge.sqliteVacuum.concierge-mailer = outbox.path;

    # MXroute's addresses change, so the mailer is held to its port.
    weblounge.egressPorts.concierge-mailer = [ 465 ];

    systemd.services.concierge-mailer = lib.mkMerge [
      outbox.unit
      {
        description = "Send the mail concierge queues";
        wantedBy = [ "multi-user.target" ];
        wants = [ "network-online.target" ];
        after = [ "network-online.target" ];
        environment = {
          CONCIERGE_OUTBOX = outbox.path;
          CONCIERGE_SMTP_HOST = cfg.server;
          CONCIERGE_SMTP_USERNAME = sender;
          CONCIERGE_MAIL_FROM = "${domain} <${sender}>";
          # Tokio starts a worker per core, which TasksMax would have to
          # follow; the mailer sends one message at a time.
          TOKIO_WORKER_THREADS = "2";
        };
        serviceConfig = config.weblounge.sandbox // serviceOnly // {
          User = "concierge-mailer";
          DynamicUser = true;

          # It talks to MXroute alone: nothing on this machine but resolved's
          # stub, and nothing on private networks.
          IPAddressDeny = [ "localhost" "link-local" "multicast" "10.0.0.0/8" "100.64.0.0/10" "172.16.0.0/12" "192.168.0.0/16" "fc00::/7" ];
          IPAddressAllow = [ "127.0.0.53" ];
          SocketBindDeny = "any";

          # The outbox, the trust store in /etc and resolved's address are
          # all it reads beyond the store.
          TemporaryFileSystem = [ "/var:ro" "/run:ro" ];
          BindPaths = [ outbox.directory ];
          BindReadOnlyPaths = [ "/run/systemd/resolve" ];
          # PrivateTmp= and PrivateDevices= mount their own /tmp and /dev/shm.
          NoExecPaths = [ "/" "/tmp" "/dev/shm" ];
          ExecPaths = [ "/nix/store" ];
          PrivateIPC = true;

          SystemCallFilter = config.weblounge.sandbox.SystemCallFilter
            ++ [ "~@chown" "~@ipc" "~@keyring" "~@memlock" "~@resources" "~@setuid" ];
          SystemCallErrorNumber = "EPERM";

          # One SMTP connection and a few to SQLite, with room to spare.
          MemoryMax = "128M";
          TasksMax = 32;
          LimitNOFILE = 128;
        };
      }
    ];

    # MXroute may block an address that fails to log in again and again,
    # so this tries for a few seconds alone.
    weblounge.deployChecks."the mailer runs and logs in to its SMTP server" = {
      timeout = 5;
      script = ''
        systemctl is-active --quiet concierge-mailer || { echo "concierge-mailer isn't running"; exit 1; }
        ${check} && exit 0
        echo "Until the real password replaces the placeholder (sops secrets/credenza.yaml), this fails."
        exit 1
      '';
    };
  };
}

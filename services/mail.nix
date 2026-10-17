{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.mail;
  inherit (config.weblounge) domain;
  sender = "noreply@${domain}";
  inherit (config.weblounge.concierge) outbox;
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
        };
        serviceConfig = config.weblounge.sandbox // {
          User = "concierge-mailer";
          DynamicUser = true;
          ExecStart = "${lib.getExe pkgs.weblounge.concierge} mailer";
          Restart = "on-failure";
        };
      }
    ];
  };
}

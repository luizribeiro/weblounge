# The greeter: concierge, weblounge's resident bot, welcomes newcomers in
# #lounge and tells their inviters. It parses what the network sends it,
# so it holds concierge's database and its account's password alone; the
# oneshot that puts the password back before every start holds Ergo's API
# token instead.
{ config, lib, pkgs, ... }:

let
  cfg = config.weblounge.greeter;
  account = "concierge-greeter-account";
  password = "concierge-greeter-password";
  ca = config.weblounge.selfSignedCa;
  inherit (config.weblounge) sandbox loopbackOnly;
  inherit (config.weblounge.concierge) database;
in
{
  imports = [ ./concierge.nix ./ergo.nix ./sandbox.nix ./secrets.nix ];

  options.weblounge.greeter.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Whether the greeter runs. VM tests but its own turn it off, so their members aren't welcomed.";
  };

  options.weblounge.greeter.weeklyHost = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = "luiz";
    description = "The member the greeter reminds on Mondays to start the week's thread in #lounge, if any.";
  };

  config = lib.mkIf cfg.enable {
    weblounge.secrets.${password} = {
      ephemeral = true;
      loadedBy = [ account "concierge-greeter" ];
    };
    weblounge.secrets.ergo-api-token.loadedBy = [ account ];

    systemd.services.${account} = lib.mkMerge [
      database.unit
      {
        description = "Register the greeter's account, or set its password";
        wants = [ "ergochat.service" ];
        after = [ "ergochat.service" ];
        environment.CONCIERGE_ERGO_API = "http://${config.weblounge.ergo.settings.api.listener}";
        script = config.weblounge.ergo.untilItAnswers
          "${lib.getExe pkgs.weblounge.concierge} ensure-greeter-account";
        serviceConfig = sandbox // loopbackOnly // {
          Type = "oneshot";
          RemainAfterExit = true;
        };
      }
    ];

    systemd.services.concierge-greeter = lib.mkMerge [
      database.unit
      {
        description = "The greeter, which welcomes newcomers in #lounge";
        wantedBy = [ "multi-user.target" ];
        requires = [ "${account}.service" ];
        wants = [ "ergochat.service" ];
        after = [ "${account}.service" "ergochat.service" ];
        # rustls trusts SSL_CERT_FILE in place of the system's CAs.
        environment = {
          CONCIERGE_IRC_SERVER = config.weblounge.ergo.localAddress;
          CONCIERGE_PUBLIC_URL = config.weblounge.concierge.publicUrl;
        } // lib.optionalAttrs (ca != null) { SSL_CERT_FILE = "%d/ca"; }
        // lib.optionalAttrs (cfg.weeklyHost != null) { CONCIERGE_GREETER_WEEKLY_HOST = cfg.weeklyHost; };
        serviceConfig = sandbox // loopbackOnly // {
          ExecStart = lib.getExe pkgs.weblounge.greeter;
          Restart = "always";
          RestartSec = 10;
          LoadCredential = lib.optional (ca != null) "ca:${ca}";
        };
      }
    ];
  };
}

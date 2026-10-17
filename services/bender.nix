# bender, the network's own bot: the bot template, deployed next to the
# Ergo and concierge it talks to.
{ config, lib, ... }:

let
  nick = "bender";
  owner = "luiz";
  account = "${nick}-account";
  secret = config.weblounge.secrets."${nick}-password";
in
{
  imports = [ ./concierge.nix ./ergo.nix ./sandbox.nix ./secrets.nix ../templates/bot/module.nix ];

  services.weblounge-bot.${nick} = {
    enable = true;
    server = config.weblounge.ergo.localAddress;
    channels = [ "#bots" ];
    passwordFile = secret.path;
    caFile = config.weblounge.selfSignedCa;
  };

  # The account puts the password back in Ergo before every start of the bot.
  weblounge.secrets."${nick}-password" = {
    ephemeral = true;
    loadedBy = [ account ];
  };

  systemd.services.${account} = {
    description = "Register ${nick}'s account, or set its password";
    wants = [ "concierge.service" "ergochat.service" ];
    after = [ "concierge.service" "ergochat.service" ];
    # concierge-admin's command runs as concierge's own user, which can't
    # open the credential, so the password reaches it on standard input.
    script = ''
      cat "$CREDENTIALS_DIRECTORY/${nick}-password" \
        | ${lib.getExe config.weblounge.concierge.admin} bot ensure ${owner} ${nick} --password-file -
    '';
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    # Its script calls concierge-admin, which changes with every change to
    # concierge; restarting it on deploy would take the bot down with it.
    # It only needs to run before the bot starts.
    restartIfChanged = false;
  };

  systemd.services."weblounge-bot-${nick}" = {
    requires = [ "${account}.service" ];
    # Its account's order after ergo doesn't hold when a deploy restarts
    # ergo and bender but not the account.
    wants = [ "ergochat.service" ];
    after = [ "${account}.service" "ergochat.service" ];
    serviceConfig = config.weblounge.loopbackOnly;
  };
}

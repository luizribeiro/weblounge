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
  imports = [ ./concierge.nix ./ergo.nix ./secrets.nix ../templates/bot/module.nix ];

  services.weblounge-bot.${nick} = {
    enable = true;
    server = config.weblounge.ergo.localAddress;
    channels = [ "#bots" ];
    passwordFile = secret.path;
    caFile = config.weblounge.selfSignedCa;
  };

  weblounge.secrets."${nick}-password".loadedBy = [ account ];

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
  };

  systemd.services."weblounge-bot-${nick}" = {
    requires = [ "${account}.service" ];
    after = [ "${account}.service" ];
  };
}

# bender, the network's own bot: louro holds its connection to the Ergo
# next to it, and lets members of the louro group speak in #ops through it.
{ config, lib, ... }:

let
  nick = "bender";
  owner = "luiz";
  account = "${nick}-account";
  secret = config.weblounge.secrets."${nick}-password";
  ca = config.weblounge.selfSignedCa;
in
{
  imports = [ ./concierge.nix ./ergo.nix ./sandbox.nix ./secrets.nix ];

  services.louro = {
    enable = true;
    settings = {
      server = {
        address = config.weblounge.ergo.localAddress;
        inherit nick;
        sasl_username = nick;
        autojoin = [ "#bots" "#ops" ];
      };
      grants.irc = {
        join = [ "#bots" "#ops" ];
        send = [ "#ops" ];
      };
    };
    passwordFile = secret.path;
  };

  # louro's commands find the socket by it, as root runs them by hand.
  environment.variables.LOURO_SOCKET = config.services.louro.socket;

  # The account puts the password back in Ergo before every start of louro.
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
    # Ergo may be too busy to answer early in a boot, and louro, which
    # needs the account, would stay down; a few tries ride that out.
    script = ''
      for wait in 15 15 15 0; do
        cat "$CREDENTIALS_DIRECTORY/${nick}-password" \
          | ${lib.getExe config.weblounge.concierge.admin} bot ensure ${owner} ${nick} --password-file - \
          && exit 0
        sleep "$wait"
      done
      exit 1
    '';
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    # Its script calls concierge-admin, which changes with every change to
    # concierge; restarting it on deploy would take bender down with it.
    # It only needs to run before louro starts.
    restartIfChanged = false;
  };

  systemd.services.louro = {
    requires = [ "${account}.service" ];
    # Its account's order after ergo doesn't hold when a deploy restarts
    # ergo and louro but not the account.
    wants = [ "ergochat.service" ];
    after = [ "${account}.service" "ergochat.service" ];
    # rustls trusts SSL_CERT_FILE in place of the system's CAs.
    environment = lib.optionalAttrs (ca != null) { SSL_CERT_FILE = "%d/ca"; };
    serviceConfig = config.weblounge.loopbackOnly // {
      LoadCredential = lib.optional (ca != null) "ca:${ca}";
      RestartSec = 10;
      RestrictSUIDSGID = true;
    };
  };
}

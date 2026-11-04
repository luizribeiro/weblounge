# bender, the network's own bot: louro holds its connection to the Ergo
# next to it, and lets members of the louro group speak in #ops through it.
# It's a house account, so nobody may take its name.
{ config, lib, ... }:

let
  nick = "bender";
  account = config.weblounge.ergo.houseAccounts.${nick};
  secret = config.weblounge.secrets.${account.password};
  ca = config.weblounge.selfSignedCa;
in
{
  imports = [ ./concierge.nix ./ergo.nix ./house-accounts.nix ./sandbox.nix ./secrets.nix ];

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

  weblounge.ergo.houseAccounts.${nick}.vhost = "weblounge/${nick}";

  # Its script runs concierge, which changes with every change to
  # concierge; restarting it on deploy would take bender down with it, as
  # louro requires it. It only needs to run before louro starts.
  systemd.services.${account.unit}.restartIfChanged = false;

  systemd.services.louro = {
    requires = [ "${account.unit}.service" ];
    # Its account's order after ergo doesn't hold when a deploy restarts
    # ergo and louro but not the account.
    wants = [ "ergochat.service" ];
    after = [ "${account.unit}.service" "ergochat.service" ];
    # rustls trusts SSL_CERT_FILE in place of the system's CAs.
    environment = lib.optionalAttrs (ca != null) { SSL_CERT_FILE = "%d/ca"; };
    serviceConfig = config.weblounge.loopbackOnly // {
      LoadCredential = lib.optional (ca != null) "ca:${ca}";
      RestartSec = 10;
      RestrictSUIDSGID = true;
    };
  };
}

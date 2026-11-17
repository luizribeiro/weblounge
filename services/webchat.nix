{ config, pkgs, ... }:

{
  imports = [ ./web-client.nix ];

  weblounge.webClients.${config.weblounge.ergo.serverName} = {
    package = pkgs.weblounge.clientRelease;
    channel = "release";
  };
}

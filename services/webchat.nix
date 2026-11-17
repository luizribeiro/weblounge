{ config, ... }:

{
  imports = [ ./web-client.nix ];

  weblounge.webClients.${config.weblounge.ergo.serverName}.channel = "release";
}

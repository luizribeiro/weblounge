{ config, pkgs, ... }:

let
  hostName = "beta.${config.weblounge.domain}";
in
{
  imports = [ ./web-client.nix ];

  weblounge.webClients.${hostName} = {
    package = pkgs.weblounge.client;
    channel = "beta";
  };
}

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

  weblounge.concierge.webClients.CONCIERGE_BETA_CLIENT_URL = config.weblounge.origin hostName;
}

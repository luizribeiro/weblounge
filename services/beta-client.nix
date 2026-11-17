{ config, ... }:

let
  hostName = "beta.${config.weblounge.domain}";
in
{
  imports = [ ./web-client.nix ];

  weblounge.webClients.${hostName}.channel = "beta";
}

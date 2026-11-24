{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;
  versions = lib.importJSON ../client-versions.json;
  served = host: weblounge.hosts.credenza.config.services.nginx.virtualHosts.${host}.root;
  isBuiltWithItsPinsVersion = host: channel: {
    expr = (served host).WEBLOUNGE_VERSION or null;
    expected = versions.${channel};
  };
in
{
  testBetaIsBuiltWithItsPinsVersion = isBuiltWithItsPinsVersion "beta.weblounge.org" "beta";
  testReleaseIsBuiltWithItsPinsVersion = isBuiltWithItsPinsVersion "irc.weblounge.org" "release";
}

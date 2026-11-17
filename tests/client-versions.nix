{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;
  versions = lib.importJSON ../client-versions.json;
  served = host: weblounge.hosts.credenza.config.services.nginx.virtualHosts.${host}.root;
  builtVersion = host: (served host).WEBLOUNGE_VERSION or null;
in
{
  testBetaIsBuiltWithItsPinsVersion = {
    expr = builtVersion "beta.weblounge.org";
    expected = versions.beta;
  };

  # The release pin predates withVersion, so its build carries no version.
  testReleaseTooOldForWithVersionIsServedAsIs = {
    expr = builtVersion "irc.weblounge.org";
    expected = null;
  };
}

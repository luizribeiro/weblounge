{ pkgs, vmTest, ... }:

let
  inherit (pkgs) lib;
  locked = input: (lib.importJSON ../../flake.lock).nodes.${input}.locked.rev;
  versions = lib.importJSON ../../client-versions.json;
  # What each channel's version.json should say, from the pins themselves.
  pins = {
    beta = { commit = locked "weblounge-client"; version = versions.beta; };
    release = { commit = locked "weblounge-client-release"; version = versions.release; };
  };
in
vmTest {
  webkit = true;
  browsers.client-signin-browser = ./clients/client-signin-browser.py;
  client.environment.etc."client-pins.json".text = builtins.toJSON pins;
}

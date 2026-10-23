# The host's major events, one plain line each in #ops, through bender's
# louro. ops-notify posts a line of root's.
{ config, pkgs, ... }:

let
  opsNotify = pkgs.callPackage ../pkgs/ops-events {
    louro = config.services.louro.package;
    inherit (config.services.louro) socket;
  };
in
{
  imports = [ ./bender.nix ];

  environment.systemPackages = [ opsNotify ];
}

{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;
  inherit (weblounge.hosts.credenza) config;

  afterOf = unit: config.systemd.services.${unit}.after;
in
{
  testVacuumsWaitForTheMigration = {
    expr = map (unit: lib.elem "concierge-migrate.service" (afterOf unit)) [ "concierge-vacuum" "concierge-mailer-vacuum" ];
    expected = [ true true ];
  };

  testVacuumsLeaveTheNetworkTargetsTheirOwnersWaitFor = {
    expr = map (unit: lib.filter (lib.hasSuffix ".target") (afterOf unit)) [ "concierge-vacuum" "concierge-mailer-vacuum" ];
    expected = [ [ ] [ ] ];
  };

  testVacuumsRaiseNoOrderingWarnings = {
    expr = lib.filter (lib.hasInfix "-vacuum.service") config.warnings;
    expected = [ ];
  };
}

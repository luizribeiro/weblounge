{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  credenzaWith = modules: (weblounge.hosts.credenza.extendModules { inherit modules; }).config;

  withMulticlient = setting: credenzaWith [{
    weblounge.ergo.settings.accounts.multiclient = lib.mapAttrs (_: lib.mkForce) setting;
  }];

  refusedAsOptional = config: lib.any
    (assertion: !assertion.assertion && lib.hasInfix "must be mandatory" assertion.message)
    config.assertions;

  ergoRestartTriggers = config: config.systemd.services.ergochat.restartTriggers;
in
{
  testCredenzaPassesConciergesMandatoryCheck = {
    expr = refusedAsOptional weblounge.hosts.credenza.config;
    expected = false;
  };

  testConciergeRefusesAnOptionalAlwaysOn = {
    expr = refusedAsOptional (withMulticlient { always-on = "opt-out"; });
    expected = true;
  };

  testConciergeRefusesAnOptionalAutoAway = {
    expr = refusedAsOptional (withMulticlient { auto-away = "opt-out"; });
    expected = true;
  };

  testChangingAlwaysOnRestartsErgo = {
    expr = ergoRestartTriggers (withMulticlient { always-on = "opt-out"; })
      == ergoRestartTriggers weblounge.hosts.credenza.config;
    expected = false;
  };
}

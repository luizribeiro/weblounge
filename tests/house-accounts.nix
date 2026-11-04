{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;
  inherit (weblounge.hosts.credenza) config;

  credenzaWith = account: weblounge.hosts.credenza.extendModules {
    modules = [{ weblounge.ergo.houseAccounts = account; }];
  };

  failedAssertions = account: map (check: check.message)
    (lib.filter (check: !check.assertion) (credenzaWith account).config.assertions);

  unit = config.systemd.services.concierge-account;
in
{
  testEachHouseAccountGetsAnOneshotThatEnsuresIt = {
    expr = {
      inherit (unit.serviceConfig) Type RemainAfterExit User;
      ensures = lib.hasInfix "ensure-house-account concierge weblounge/concierge" unit.script;
    };
    expected = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "concierge";
      ensures = true;
    };
  };

  testTheOneshotHoldsErgosTokenAndSharesThePasswordWithTheGreeter = {
    expr = {
      ergoToken = lib.elem "concierge-account" config.weblounge.secrets.ergo-api-token.loadedBy;
      password = lib.sort lib.lessThan config.weblounge.secrets.concierge-password.loadedBy;
      ephemeral = config.weblounge.secrets.concierge-password.ephemeral;
    };
    expected = {
      ergoToken = true;
      password = [ "concierge-account" "concierge-greeter" ];
      ephemeral = true;
    };
  };

  testValidHouseAccountsPass = {
    expr = failedAssertions { example.vhost = "weblounge/example"; };
    expected = [ ];
  };

  testVhostsFollowErgosPattern = {
    expr = failedAssertions { example.vhost = "weblounge example"; };
    expected = [ "House account example's vhost weblounge example must be 1 to 64 letters, digits and . - _ /, as Ergo accepts." ];
  };

  testVhostsFitErgosLimit = {
    expr = lib.length (failedAssertions { example.vhost = lib.concatStrings (lib.replicate 65 "w"); });
    expected = 1;
  };

  testNamesFollowTheRuleForNicks = {
    expr = failedAssertions { "2pac".vhost = "weblounge/2pac"; };
    expected = [ "House account 2pac's name must follow the rule for nicks: a letter, then up to 31 letters, digits, hyphens and underscores." ];
  };
}

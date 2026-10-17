{ pkgs, weblounge, ... }:

let
  inherit (pkgs) lib;

  credenzaWith = secret: weblounge.hosts.credenza.extendModules {
    modules = [{ weblounge.secrets.example = secret; }];
  };

  pathOf = secret: (credenzaWith secret).config.weblounge.secrets.example.path;

  failedAssertions = secret: map (check: check.message)
    (lib.filter (check: !check.assertion) (credenzaWith secret).config.assertions);
in
{
  testEphemeralSecretsLiveInRam = {
    expr = pathOf { ephemeral = true; };
    expected = "/run/weblounge-secrets/example";
  };

  testOtherGeneratedSecretsLiveOnDisk = {
    expr = pathOf { };
    expected = "/var/lib/weblounge-secrets/example";
  };

  testSopsSecretsCantBeEphemeral = {
    expr = failedAssertions { ephemeral = true; fromSops = true; };
    expected = [ "weblounge.secrets.example comes from sops, so it can't be generated anew at each boot." ];
  };
}

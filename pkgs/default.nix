{ crane, advisory-db, weblounge-client }:

final: prev:
let
  clientPackages = weblounge-client.packages.${final.stdenv.hostPlatform.system};
in
{
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    gamja = final.callPackage ./gamja.nix { };
    client = clientPackages.app;
    inherit (clientPackages) push-relay;
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber greeter;
  };
}

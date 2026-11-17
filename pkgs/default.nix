{ crane, advisory-db, weblounge-client, weblounge-client-release }:

final: prev:
let
  packagesOf = flake: flake.packages.${final.stdenv.hostPlatform.system};
  clientPackages = packagesOf weblounge-client;
in
{
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    gamja = final.callPackage ./gamja.nix { };
    client = clientPackages.app;
    clientRelease = (packagesOf weblounge-client-release).app;
    inherit (clientPackages) push-relay;
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber greeter;
  };
}

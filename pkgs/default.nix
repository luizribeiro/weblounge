{ crane, advisory-db, weblounge-client, weblounge-client-release }:

final: prev:
let
  packagesOf = flake: flake.packages.${final.stdenv.hostPlatform.system};
  releasePackages = packagesOf weblounge-client-release;
in
{
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    client = (packagesOf weblounge-client).app;
    clientRelease = releasePackages.app;
    inherit (releasePackages) push-relay;
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber greeter;
  };
}

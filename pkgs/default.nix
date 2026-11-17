{ crane, advisory-db, weblounge-client, weblounge-client-release }:

final: prev:
let
  packagesOf = flake: flake.packages.${final.stdenv.hostPlatform.system};
  clientPackages = packagesOf weblounge-client;
  versions = final.lib.importJSON ../client-versions.json;

  # A flake input carries no tags, so each pin's version comes from
  # client-versions.json. A build too old for withVersion is served as is.
  clientOf = flake: version:
    let app = (packagesOf flake).app;
    in {
      package = if version != null && app ? withVersion then app.withVersion version else app;
      commit = flake.rev;
      inherit version;
    };
in
{
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    clients = {
      beta = clientOf weblounge-client versions.beta;
      release = clientOf weblounge-client-release versions.release;
    };
    inherit (clientPackages) push-relay;
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber greeter;
  };
}

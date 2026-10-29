{ crane, advisory-db, weblounge-client }:

final: prev: {
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    gamja = final.callPackage ./gamja.nix { };
    client = weblounge-client.packages.${final.stdenv.hostPlatform.system}.app;
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber greeter;
  };
}

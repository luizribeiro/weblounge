{ crane, advisory-db }:

final: prev: {
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    gamja = final.callPackage ./gamja.nix { };
    ergoConfig = final.callPackage ./ergo-config.nix { };
    inherit (final.callPackage ./workspace.nix { craneLib = crane.mkLib final; inherit advisory-db; }) concierge mailer scrubber;
  };
}

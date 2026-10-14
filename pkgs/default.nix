{ crane }:

final: prev: {
  ergochat = final.callPackage ./ergochat.nix { inherit (prev) ergochat; };

  weblounge = {
    web = final.callPackage ./web.nix { };
    concierge = final.callPackage ./concierge.nix { craneLib = crane.mkLib final; };
  };
}

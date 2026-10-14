{
  description = "An IRC bot for weblounge.org";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      inherit (nixpkgs) lib;
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSystem = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs:
        let bot = pkgs.callPackage ./package.nix { };
        in {
          default = bot;
        } // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          image = pkgs.dockerTools.buildLayeredImage {
            name = "bot";
            tag = "latest";
            config = {
              Entrypoint = [ (lib.getExe bot) ];
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
              User = "65534:65534";
            };
          };
        });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell { packages = [ pkgs.go pkgs.gopls ]; };
      });
    };
}

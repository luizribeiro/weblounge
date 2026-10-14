{
  description = "weblounge.org: an invite-only IRC network";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, git-hooks, ... } @ inputs:
    let
      inherit (nixpkgs) lib;
      weblounge = import ./lib inputs;
      devSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forEachSystem = f: lib.genAttrs devSystems
        (system: f (nixpkgs.legacyPackages.${system}.extend weblounge.overlay));
      gitHooks = forEachSystem (pkgs: import ./git-hooks.nix { inherit git-hooks pkgs; });
    in
    {
      nixosConfigurations = weblounge.hosts;

      overlays.default = weblounge.overlay;

      packages = lib.recursiveUpdate weblounge.vmPackages
        (forEachSystem (pkgs: { inherit (pkgs.weblounge) web concierge; }));

      checks = lib.foldl' lib.recursiveUpdate { } [
        weblounge.toplevelChecks
        (lib.mapAttrs (_: hooks: { git-hooks = hooks; }) gitHooks)
        (forEachSystem (pkgs: {
          web-typecheck = pkgs.weblounge.web.typecheck;
        } // lib.mapAttrs' (name: lib.nameValuePair "concierge-${name}") pkgs.weblounge.concierge.checks))
        {
          x86_64-linux = lib.mapAttrs
            (_: test: import test {
              inherit weblounge;
              pkgs = nixpkgs.legacyPackages.x86_64-linux;
            })
            {
              credenza = ./tests/credenza.nix;
              ergo-api-loopback = ./tests/ergo-api-loopback.nix;
            };
        }
      ];

      formatter = forEachSystem (pkgs: pkgs.nixpkgs-fmt);

      devShells = forEachSystem (pkgs: {
        default = import ./shell.nix {
          inherit pkgs;
          gitHooks = gitHooks.${pkgs.stdenv.hostPlatform.system};
        };
      });
    };
}

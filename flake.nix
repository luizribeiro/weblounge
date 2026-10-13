{
  description = "weblounge.org: an invite-only IRC network";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, ... } @ inputs:
    let
      inherit (nixpkgs) lib;
      weblounge = import ./lib inputs;
    in
    {
      nixosConfigurations = weblounge.hosts;

      packages = weblounge.vmPackages;

      checks = lib.foldl' lib.recursiveUpdate { } [
        weblounge.toplevelChecks
        {
          x86_64-linux.credenza = import ./tests/credenza.nix {
            inherit weblounge;
            pkgs = nixpkgs.legacyPackages.x86_64-linux;
          };
        }
      ];
    };
}

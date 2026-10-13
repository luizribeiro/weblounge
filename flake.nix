{
  description = "weblounge.org: an invite-only IRC network";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs:
    let
      weblounge = import ./lib inputs;
    in
    {
      nixosConfigurations = weblounge.hosts;

      checks = weblounge.toplevelChecks;
    };
}

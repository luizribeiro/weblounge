{
  description = "weblounge.org: an invite-only IRC network";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
    # RustSec's advisories, which concierge-audit checks Cargo.lock against.
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    sops-nix = {
      url = "github:Mic92/sops-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # The daemon that holds bender's IRC connection.
    louro = {
      url = "github:luizribeiro/louro";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.crane.follows = "crane";
      inputs.git-hooks.follows = "git-hooks";
    };
  };

  outputs = { nixpkgs, git-hooks, ... } @ inputs:
    let
      inherit (nixpkgs) lib;
      weblounge = import ./lib inputs;
      devSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forEachSystem = f: lib.genAttrs devSystems
        (system: f (nixpkgs.legacyPackages.${system}.extend weblounge.overlay));
      dnsconfig = pkgs: pkgs.callPackage ./dns { inherit (weblounge) zone; };
      gitHooks = forEachSystem (pkgs: import ./git-hooks.nix { inherit git-hooks pkgs; });
    in
    {
      nixosConfigurations = weblounge.hosts;

      overlays.default = weblounge.overlay;

      templates.bot = {
        path = ./templates/bot;
        description = "An IRC bot for weblounge.org, in Go";
      };

      packages = lib.recursiveUpdate weblounge.vmPackages
        (forEachSystem (pkgs: {
          inherit (pkgs.weblounge) web concierge mailer scrubber greeter;
          dnsconfig = dnsconfig pkgs;
        }));

      checks = lib.foldl' lib.recursiveUpdate { } [
        weblounge.toplevelChecks
        # The sandbox can't fetch crates; concierge-clippy lints with them.
        (lib.mapAttrs (_: hooks: { git-hooks = hooks.overrideAttrs { SKIP = "clippy"; }; }) gitHooks)
        (forEachSystem (pkgs: {
          # Building it runs its go test.
          bot-template-package = pkgs.callPackage ./templates/bot/package.nix { };
          # Building it runs dnscontrol check on the zone.
          dnsconfig = dnsconfig pkgs;
          felt-seams = pkgs.weblounge.web.feltSeams;
        } // lib.mapAttrs' (name: lib.nameValuePair "concierge-${name}") pkgs.weblounge.concierge.checks))
        weblounge.testChecks
      ];

      # `just nix-unit` runs them, with diffs of what failed.
      tests = weblounge.unitTests;

      formatter = forEachSystem (pkgs: pkgs.nixpkgs-fmt);

      devShells = forEachSystem (pkgs: {
        default = import ./shell.nix {
          inherit pkgs;
          gitHooks = gitHooks.${pkgs.stdenv.hostPlatform.system};
          ergoSettings = weblounge.hosts.credenza.config.weblounge.ergo.settings;
        };
      });
    };
}

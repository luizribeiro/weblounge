{ self, nixpkgs, disko, crane, ... }:

let
  inherit (nixpkgs) lib;

  overlay = import ../pkgs { inherit crane; };

  hostNames = builtins.attrNames
    (lib.filterAttrs (_: type: type == "directory") (builtins.readDir ../hosts));

  hostModules = name: [
    disko.nixosModules.disko
    ../modules
    ../hosts/${name}
    {
      networking.hostName = name;
      nixpkgs.overlays = [ overlay ];
    }
  ];

  hosts = lib.genAttrs hostNames
    (name: lib.nixosSystem { modules = hostModules name; });

  systemOf = host: host.pkgs.stdenv.hostPlatform.system;

  perHostOutput = mkOutput: lib.foldl' lib.recursiveUpdate { }
    (lib.mapAttrsToList
      (name: host: { ${systemOf host} = mkOutput name host; })
      self.nixosConfigurations);

  testErgo = import ./test-ergo.nix {
    inherit lib;
    inherit (hosts.credenza.config.weblounge.ergo) settings;
  };

  zone = import ../dns/zone.nix { inherit lib hosts; };

  weblounge = { inherit overlay hosts hostModules testErgo zone; };

  nixFilesIn = dir: lib.mapAttrs' (file: _: lib.nameValuePair (lib.removeSuffix ".nix" file) (dir + "/${file}"))
    (lib.filterAttrs (file: type: type == "regular" && lib.hasSuffix ".nix" file) (builtins.readDir dir));

  tests = nixFilesIn ../tests // nixFilesIn ../tests/e2e;

  checkOf = pkgs: name: file: import file {
    inherit pkgs weblounge;
    vmTest = import ../tests/e2e/lib/lib.nix { inherit pkgs weblounge name; };
  };
in
weblounge // {
  vmPackages = perHostOutput (name: host: {
    "${name}-vm" = host.config.virtualisation.vmVariant.system.build.vm;
  });

  toplevelChecks = perHostOutput (name: host: {
    "toplevel-${name}" = host.config.system.build.toplevel;
  });

  testChecks.x86_64-linux = lib.mapAttrs
    (checkOf nixpkgs.legacyPackages.x86_64-linux)
    tests;
}

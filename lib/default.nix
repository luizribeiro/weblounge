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

  filesIn = suffix: dir: lib.mapAttrs' (file: _: lib.nameValuePair (lib.removeSuffix suffix file) (dir + "/${file}"))
    (lib.filterAttrs (file: type: type == "regular" && lib.hasSuffix suffix file) (builtins.readDir dir));

  # A VM test is tests/e2e/<name>.py, with a <name>.nix beside it if it
  # needs more than lib.nix gives it.
  vmTests = lib.mapAttrs (_: _: { vmTest, ... }: vmTest { }) (filesIn ".py" ../tests/e2e)
    // lib.mapAttrs (_: import) (filesIn ".nix" ../tests/e2e);

  tests = lib.mapAttrs (_: import) (filesIn ".nix" ../tests) // vmTests;

  checkOf = pkgs: name: test: test {
    inherit pkgs weblounge;
    vmTest = import ../tests/e2e/lib/lib.nix {
      inherit pkgs weblounge name;
      script = ../tests/e2e/${name}.py;
    };
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

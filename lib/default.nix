{ self, nixpkgs, disko, sops-nix, louro, crane, advisory-db, ... }:

let
  inherit (nixpkgs) lib;

  overlay = import ../pkgs { inherit crane advisory-db; };

  hostNames = builtins.attrNames
    (lib.filterAttrs (_: type: type == "directory") (builtins.readDir ../hosts));

  hostModules = name: [
    disko.nixosModules.disko
    sops-nix.nixosModules.sops
    louro.nixosModules.default
    ../modules
    ../hosts/${name}
    {
      networking.hostName = name;
      nixpkgs.overlays = [ overlay ];
    }
  ];

  # Tests build from hostModules alone, so a new commit doesn't rebuild
  # every VM test.
  revision.system.configurationRevision = self.rev or self.dirtyRev or null;

  hosts = lib.genAttrs hostNames
    (name: lib.nixosSystem { modules = hostModules name ++ [ revision ]; });

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

  testPkgs = nixpkgs.legacyPackages.x86_64-linux;

  # Each test is a derivation, or a set of nix-unit tests for evalCheck.
  results = lib.mapAttrs
    (name: test: test {
      pkgs = testPkgs;
      inherit weblounge;
      vmTest = import ../tests/e2e/lib/lib.nix {
        pkgs = testPkgs;
        inherit weblounge name;
        script = ../tests/e2e/${name}.py;
      };
    })
    tests;

  # nixpkgs' runTests reads nix-unit's format, without nix-unit's need to
  # run Nix inside the build sandbox.
  evalCheck = name: unitTests:
    let
      failures = lib.debug.runTests unitTests;
    in
    testPkgs.runCommand name { } (
      if failures == [ ] then "touch $out"
      else "echo ${lib.escapeShellArg (lib.generators.toPretty { } failures)} >&2; echo 'just nix-unit shows diffs.' >&2; exit 1"
    );
in
weblounge // {
  vmPackages = perHostOutput (name: host: {
    "${name}-vm" = host.config.virtualisation.vmVariant.system.build.vm;
  });

  toplevelChecks = perHostOutput (name: host: {
    "toplevel-${name}" = host.config.system.build.toplevel;
  });

  testChecks.x86_64-linux = lib.mapAttrs
    (name: result: if lib.isDerivation result then result else evalCheck name result)
    results;

  unitTests = lib.filterAttrs (_: result: !lib.isDerivation result) results;
}

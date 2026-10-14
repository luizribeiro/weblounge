{ self, nixpkgs, disko, ... }:

let
  inherit (nixpkgs) lib;

  overlay = import ../pkgs;

  hostNames = builtins.attrNames
    (lib.filterAttrs (_: type: type == "directory") (builtins.readDir ../hosts));

  hostModules = name: [
    disko.nixosModules.disko
    ../modules
    ../hosts/${name}
    { networking.hostName = name; }
  ];

  hosts = lib.genAttrs hostNames
    (name: lib.nixosSystem { modules = hostModules name; });

  systemOf = host: host.pkgs.stdenv.hostPlatform.system;

  perHostOutput = mkOutput: lib.foldl' lib.recursiveUpdate { }
    (lib.mapAttrsToList
      (name: host: { ${systemOf host} = mkOutput name host; })
      self.nixosConfigurations);
in
{
  inherit overlay hosts hostModules;

  vmPackages = perHostOutput (name: host: {
    "${name}-vm" = host.config.virtualisation.vmVariant.system.build.vm;
  });

  toplevelChecks = perHostOutput (name: host: {
    "toplevel-${name}" = host.config.system.build.toplevel;
  });
}

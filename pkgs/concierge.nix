{ lib, craneLib }:

let
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../crates ];
  };

  commonArgs = {
    inherit src;
    inherit (craneLib.crateNameFromCargoToml { cargoToml = ../crates/concierge/Cargo.toml; }) pname version;
    strictDeps = true;
  };

  # Dependencies build once here, so editing our code doesn't rebuild them.
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  withDependencies = args: commonArgs // args // { inherit cargoArtifacts; };
in
craneLib.buildPackage (withDependencies {
  doCheck = false;
  meta.mainProgram = "concierge";

  passthru.checks = {
    clippy = craneLib.cargoClippy (withDependencies {
      cargoClippyExtraArgs = "--all-targets -- --deny warnings";
    });
    test = craneLib.cargoTest (withDependencies { });
    fmt = craneLib.cargoFmt commonArgs;
  };
})

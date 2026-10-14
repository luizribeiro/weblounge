{ lib, craneLib, ergochat, weblounge }:

let
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../crates ];
  };

  commonArgs = {
    inherit src;
    inherit (craneLib.crateNameFromCargoToml { cargoToml = ../crates/concierge/Cargo.toml; }) pname version;
    strictDeps = true;
    # sqlx checks queries against the metadata in .sqlx, not a live database.
    SQLX_OFFLINE = "true";
  };

  # Dependencies build once here, so editing our code doesn't rebuild them.
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  withDependencies = args: commonArgs // args // { inherit cargoArtifacts; };

  # ergo-api's contract tests run the real Ergo on the base config production
  # builds on. nixpkgs' Ergo ships no translations for it to load.
  ergoContractTestEnv = {
    ERGO_BIN = lib.getExe ergochat;
    ERGO_CONFIG = weblounge.ergoConfig { languages.enabled = false; };
  };
in
craneLib.buildPackage (withDependencies {
  cargoExtraArgs = "--locked --package concierge";
  doCheck = false;
  meta.mainProgram = "concierge";

  passthru.ergoContractTestEnv = ergoContractTestEnv;
  passthru.checks = {
    clippy = craneLib.cargoClippy (withDependencies {
      cargoClippyExtraArgs = "--all-targets -- --deny warnings";
    });
    test = craneLib.cargoTest (withDependencies ergoContractTestEnv);
    fmt = craneLib.cargoFmt commonArgs;
  };
})

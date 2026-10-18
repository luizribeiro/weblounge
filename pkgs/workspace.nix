{ lib, craneLib, advisory-db, ergochat, weblounge }:

let
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [ ../Cargo.toml ../Cargo.lock ../.sqlx ../crates ];
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

  # Each binary in the workspace, built on the same dependencies.
  binary = name: args: craneLib.buildPackage (withDependencies ({
    inherit (craneLib.crateNameFromCargoToml { cargoToml = ../crates/${name}/Cargo.toml; }) pname version;
    cargoExtraArgs = "--locked --package ${name}";
    doCheck = false;
    meta.mainProgram = name;
  } // args));
in
{
  concierge = binary "concierge" {
    passthru.ergoContractTestEnv = ergoContractTestEnv;
    # They cover the whole workspace.
    passthru.checks = {
      clippy = craneLib.cargoClippy (withDependencies {
        cargoClippyExtraArgs = "--all-targets -- --deny warnings";
      });
      test = craneLib.cargoTest (withDependencies ergoContractTestEnv);
      fmt = craneLib.cargoFmt commonArgs;
      audit = craneLib.cargoAudit {
        inherit src advisory-db;
        # sqlx's lock file lists every driver; RUSTSEC-2023-0071 is in rsa,
        # which only sqlx-mysql uses, and concierge doesn't build it.
        cargoAuditExtraArgs = "--ignore yanked --ignore RUSTSEC-2023-0071";
      };
    };
  };
  mailer = binary "mailer" { };
}

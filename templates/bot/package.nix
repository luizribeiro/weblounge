{ lib, buildGoModule }:

buildGoModule {
  pname = "bot";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./go.mod ./go.sum (lib.fileset.fileFilter (file: file.hasExt "go") ./.) ];
  };

  # Update after changing go.mod: set it to lib.fakeHash, build, and copy
  # the hash the error reports.
  vendorHash = "sha256-OPSr1IC/aTGR7rOzkoOdGcbEqwXZAcghpBF0a20wpDc=";

  # Static, and resolving names without NSS, which module.nix's sandbox
  # relies on.
  env.CGO_ENABLED = 0;

  meta.mainProgram = "bot";
}

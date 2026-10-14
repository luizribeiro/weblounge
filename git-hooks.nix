{ git-hooks, pkgs }:

git-hooks.lib.${pkgs.stdenv.hostPlatform.system}.run {
  src = ./.;
  hooks = {
    check-merge-conflicts.enable = true;
    end-of-file-fixer.enable = true;
    trim-trailing-whitespace = {
      enable = true;
      # A diff's blank context lines are a single space.
      excludes = [ "\\.patch$" ];
      stages = [ "pre-commit" ];
    };

    nixpkgs-fmt.enable = true;
    deadnix = {
      enable = true;
      settings = {
        noLambdaArg = true;
        noLambdaPatternNames = true;
      };
    };
    rustfmt.enable = true;
    clippy = {
      enable = true;
      settings = {
        denyWarnings = true;
        extraArgs = "--all-targets";
      };
    };
    gofmt.enable = true;
    ruff.enable = true;
    ruff-format.enable = true;
    shellcheck.enable = true;
    yamllint = {
      enable = true;
      # sops writes these, one long line per encrypted value.
      excludes = [ "^secrets/" ];
    };
    actionlint.enable = true;
    tsc = {
      enable = true;
      name = "tsc";
      entry = "${pkgs.typescript}/bin/tsc -p web/tsconfig.json";
      files = "^web/.*\\.ts$";
      pass_filenames = false;
    };
    biome = {
      enable = true;
      types_or = [ "ts" "css" "json" ];
      settings.flags = "--no-errors-on-unmatched";
    };
    ty = {
      enable = true;
      name = "ty";
      entry = "${pkgs.ty}/bin/ty check --python ${import ./tests/e2e/lib/python.nix { inherit pkgs; }}";
      files = "\\.pyi?$";
      pass_filenames = false;
    };
  };
}

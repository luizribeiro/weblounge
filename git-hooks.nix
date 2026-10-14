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
    gofmt.enable = true;
    shellcheck.enable = true;
    yamllint.enable = true;
    actionlint.enable = true;
    biome = {
      enable = true;
      types_or = [ "ts" "css" "json" ];
      settings.flags = "--no-errors-on-unmatched";
    };

    flake-check = {
      enable = true;
      name = "nix flake check";
      entry = "${pkgs.just}/bin/just check";
      pass_filenames = false;
      always_run = true;
      stages = [ "pre-push" ];
    };
  };
}

{ git-hooks, pkgs }:

git-hooks.lib.${pkgs.stdenv.hostPlatform.system}.run {
  src = ./.;
  hooks = {
    check-merge-conflicts.enable = true;
    end-of-file-fixer.enable = true;
    trim-trailing-whitespace.enable = true;

    nixpkgs-fmt.enable = true;
    deadnix = {
      enable = true;
      settings = {
        noLambdaArg = true;
        noLambdaPatternNames = true;
      };
    };
    shellcheck.enable = true;
    yamllint.enable = true;

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

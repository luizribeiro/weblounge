{ pkgs, gitHooks, ergoSettings, houseAccountNames }:

pkgs.mkShell ({
  shellHook = gitHooks.shellHook + ''
    # git-hooks.nix points core.hooksPath at .git/hooks relative to the working
    # tree, which a worktree, whose .git is a file, doesn't have.
    git config --local core.hooksPath "$(git rev-parse --path-format=absolute --git-common-dir)/hooks"
    ln -sfn ${import ./tests/e2e/lib/python.nix { inherit pkgs; }} "$(git rev-parse --show-toplevel)/.python"
  '';
  packages = gitHooks.enabledPackages ++ (with pkgs; [
    bacon
    cargo
    clippy
    dig
    dnscontrol
    esbuild
    just
    nil
    nix-unit
    nixos-anywhere
    rust-analyzer
    rustc
    rustfmt
    sops
    sqlx-cli
    typescript
  ]);
  RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
  # `just concierge dev` gives its Ergo these, so it follows the network.
  DEV_ERGO_ALWAYS_ON = ergoSettings.accounts.multiclient.always-on;
  DEV_ERGO_AUTO_AWAY = ergoSettings.accounts.multiclient.auto-away;

  CONCIERGE_HISTORY_EXPIRE_TIME = ergoSettings.history.restrictions.expire-time;
  CONCIERGE_HOUSE_ACCOUNTS = houseAccountNames;
} // pkgs.weblounge.concierge.ergoContractTestEnv)

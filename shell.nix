{ pkgs, gitHooks, multiclient }:

pkgs.mkShell ({
  shellHook = gitHooks.shellHook + ''
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
    nixos-anywhere
    rust-analyzer
    rustc
    rustfmt
    sops
    sqlx-cli
    typescript
  ]);
  RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
  # `just concierge dev` gives its Ergo these too, so both follow the network.
  CONCIERGE_ALWAYS_ON_DEFAULT = multiclient.always-on;
  CONCIERGE_AUTO_AWAY_DEFAULT = multiclient.auto-away;
} // pkgs.weblounge.concierge.ergoContractTestEnv)

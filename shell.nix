{ pkgs, gitHooks, multiclient }:

pkgs.mkShell ({
  inherit (gitHooks) shellHook;
  packages = gitHooks.enabledPackages ++ (with pkgs; [
    bacon
    cargo
    clippy
    dnscontrol
    esbuild
    just
    nil
    nixos-anywhere
    rust-analyzer
    rustc
    rustfmt
    sqlx-cli
    typescript
  ]);
  RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
  # `just concierge dev` gives its Ergo these too, so both follow the network.
  CONCIERGE_ALWAYS_ON_DEFAULT = multiclient.always-on;
  CONCIERGE_AUTO_AWAY_DEFAULT = multiclient.auto-away;
} // pkgs.weblounge.concierge.ergoContractTestEnv)

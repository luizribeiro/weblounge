{ pkgs, gitHooks }:

pkgs.mkShell ({
  inherit (gitHooks) shellHook;
  packages = gitHooks.enabledPackages ++ (with pkgs; [
    bacon
    cargo
    clippy
    esbuild
    just
    nil
    nixos-anywhere
    rust-analyzer
    rustc
    rustfmt
    typescript
  ]);
  RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
} // pkgs.weblounge.concierge.ergoContractTestEnv)

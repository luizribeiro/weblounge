{ pkgs, gitHooks }:

pkgs.mkShell {
  inherit (gitHooks) shellHook;
  packages = gitHooks.enabledPackages ++ (with pkgs; [
    esbuild
    just
    nil
    nixos-anywhere
    typescript
  ]);
}

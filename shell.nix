{ pkgs, gitHooks }:

pkgs.mkShell {
  inherit (gitHooks) shellHook;
  packages = gitHooks.enabledPackages ++ (with pkgs; [
    just
    nil
    nixos-anywhere
  ]);
}

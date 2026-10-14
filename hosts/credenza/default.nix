{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./networking.nix
    ../../services/landing.nix
    ../../services/ergo.nix
  ];

  system.stateVersion = "26.05";
}

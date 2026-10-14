{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./networking.nix
    ../../services/landing.nix
  ];

  system.stateVersion = "26.05";
}

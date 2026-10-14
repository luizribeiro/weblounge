{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./networking.nix
    ../../services/landing.nix
    ../../services/ergo.nix
    ../../services/concierge.nix
  ];

  system.stateVersion = "26.05";
}

{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./networking.nix
    ../../services/landing.nix
    ../../services/ergo.nix
    ../../services/concierge.nix
    ../../services/webchat.nix
    ../../services/bender.nix
  ];

  system.stateVersion = "26.05";
}

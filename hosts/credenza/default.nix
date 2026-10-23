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
    ../../services/ops-events.nix
    ../../services/mail.nix
  ];

  system.stateVersion = "26.05";
}

{
  imports = [
    ./hardware.nix
    ./disko.nix
    ./networking.nix
    ./nginx.nix
  ];

  system.stateVersion = "26.05";
}
